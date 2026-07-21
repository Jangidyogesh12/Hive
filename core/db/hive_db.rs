use crate::errors::DbError;
use crate::storage::label_store::LabelStore;
use crate::storage::overflow_store::OverflowStore;
use crate::storage::page::format::{META_PAGE_ID, PAGE_SIZE, PageType};
use crate::storage::page::layout;
use crate::storage::page::record::{EdgeRecord, NodeRecord, PropertyEntry};
use crate::storage::pager::Pager;
use crate::transaction::Transaction;
use crate::types::{EdgeId, NodeId, pack_record_id, unpack_record_id};
use crate::value::{self, Value};
use crate::wal::Wal;
use crate::wal::recovery::{self, RecoveryOutcome};
use crate::wal::wal_entry::{TxId, WalEntry};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, path::Path};

pub struct HiveDb {
    pub(crate) pager: Pager,
    pub(crate) wal: Wal,
    next_tx_id: AtomicU64,
    commits_since_checkpoint: u64,
    auto_checkpoint_interval: u64,
}

pub(crate) struct BeforeImage {
    page_id: u32,
    bytes: [u8; PAGE_SIZE],
    newly_allocated: bool,
}

const DEFAULT_AUTO_CHECKPOINT_INTERVAL: u64 = 64;

impl HiveDb {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        fs::create_dir_all(path).map_err(|_| DbError::FileOpenError)?;

        let wal_path = path.join("wal.hive");
        let mut pager = Pager::open(path, 128, 128)?;
        let wal = Wal::open(&wal_path)?;

        let recovery_outcome = recovery::recover(path, &mut pager)?;

        match recovery_outcome {
            RecoveryOutcome::Clean => {}
            RecoveryOutcome::Recovered {
                committed_tx_count,
                pages_redone,
            } => {
                eprintln!(
                    "Recovery: {} transactions replayed, {} pages redone",
                    committed_tx_count, pages_redone
                );
            }
        }

        Ok(Self {
            pager,
            wal,
            next_tx_id: AtomicU64::new(1),
            commits_since_checkpoint: 0,
            auto_checkpoint_interval: DEFAULT_AUTO_CHECKPOINT_INTERVAL,
        })
    }

    /// Registers a label name and returns its numeric ID.
    pub fn register_label(&mut self, name: &str) -> Result<u32, DbError> {
        LabelStore::register_label(&mut self.pager, name)
    }

    /// Returns the label name for a given ID.
    pub fn get_label_name(&mut self, label_id: u32) -> Result<Option<String>, DbError> {
        LabelStore::get_label_name(&mut self.pager, label_id)
    }

    /// Creates a new node and returns its packed NodeId.
    pub fn create_node(&mut self) -> Result<NodeId, DbError> {
        self.create_node_with_label(0)
    }

    /// Creates a new node with a label and returns its packed NodeId.
    pub fn create_node_with_label(&mut self, label_id: u32) -> Result<NodeId, DbError> {
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();

        match self.create_node_with_label_inner(label_id, Some(&mut before_images)) {
            Ok(node_id) => match self.commit_tx(tx_id) {
                Ok(()) => Ok(node_id),
                Err(err) => {
                    self.rollback_pages(&before_images)?;
                    Err(err)
                }
            },
            Err(err) => {
                self.rollback_pages(&before_images)?;
                Err(err)
            }
        }
    }

    pub(crate) fn create_node_with_label_inner(
        &mut self,
        label_id: u32,
        mut before_images: Option<&mut Vec<BeforeImage>>,
    ) -> Result<NodeId, DbError> {

        let node_id_counter = {
            let meta_page = self.pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.node_count + 1
        };

        let page_id = self.find_or_alloc_data_node_page(&mut before_images)?;

        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        let mut record = NodeRecord::new(node_id_counter);
        record.label_id = label_id;
        let mut record_buf = vec![0u8; record.encoded_size()];
        record.to_bytes(&mut record_buf)?;
        let slot = layout::insert_record(page_buf, &record_buf)?;

        self.update_meta_node_count(node_id_counter, &mut before_images)?;

        Ok(pack_record_id(page_id, slot.0))
    }

    /// Reads a node by its packed NodeId.
    pub fn get_node(&mut self, node_id: NodeId) -> Result<NodeRecord, DbError> {
        let (page_id, slot_id) = unpack_record_id(node_id);

        if slot_id == u16::MAX {
            return Err(DbError::ReadError);
        }

        let page_buf = self.pager.get_page(page_id)?;
        let record_bytes =
            layout::read_record_bytes(page_buf, slot_id).ok_or(DbError::ReadError)?;

        NodeRecord::from_bytes(record_bytes)
    }

    /// Creates an edge from src to dst and returns its packed EdgeId.
    pub fn create_edge(&mut self, src_id: NodeId, dst_id: NodeId) -> Result<EdgeId, DbError> {
        self.create_edge_with_label(src_id, dst_id, 0)
    }

    /// Creates an edge with a label from src to dst and returns its packed EdgeId.
    pub fn create_edge_with_label(
        &mut self,
        src_id: NodeId,
        dst_id: NodeId,
        label_id: u32,
    ) -> Result<EdgeId, DbError> {
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();

        match self.create_edge_with_label_inner(src_id, dst_id, label_id, Some(&mut before_images)) {
            Ok(edge_id) => match self.commit_tx(tx_id) {
                Ok(()) => Ok(edge_id),
                Err(err) => {
                    self.rollback_pages(&before_images)?;
                    Err(err)
                }
            },
            Err(err) => {
                self.rollback_pages(&before_images)?;
                Err(err)
            }
        }
    }

    pub(crate) fn create_edge_with_label_inner(
        &mut self,
        src_id: NodeId,
        dst_id: NodeId,
        label_id: u32,
        mut before_images: Option<&mut Vec<BeforeImage>>,
    ) -> Result<EdgeId, DbError> {

        let edge_id_counter = {
            let meta_page = self.pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.edge_count + 1
        };

        let page_id = self.find_or_alloc_data_edge_page(&mut before_images)?;

        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        let mut edge = EdgeRecord::new(edge_id_counter);
        edge.src = src_id;
        edge.dst = dst_id;
        edge.label_id = label_id;

        let mut record_buf = vec![0u8; edge.encoded_size()];
        edge.to_bytes(&mut record_buf)?;
        let slot = layout::insert_record(page_buf, &record_buf)?;

        self.update_meta_edge_count(edge_id_counter, &mut before_images)?;

        Ok(pack_record_id(page_id, slot.0))
    }

    /// Reads an edge by its packed EdgeId.
    pub fn get_edge(&mut self, edge_id: EdgeId) -> Result<EdgeRecord, DbError> {
        let (page_id, slot_id) = unpack_record_id(edge_id);

        if slot_id == u16::MAX {
            return Err(DbError::ReadError);
        }

        let page_buf = self.pager.get_page(page_id)?;
        let record_bytes =
            layout::read_record_bytes(page_buf, slot_id).ok_or(DbError::ReadError)?;

        EdgeRecord::from_bytes(record_bytes)
    }

    /// Sets a property on a node. Updates or appends the property entry.
    /// Long strings (> 15 bytes) are stored in overflow pages.
    pub fn set_node_property(
        &mut self,
        node_id: NodeId,
        key: &str,
        value: &Value,
    ) -> Result<(), DbError> {
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();

        match self.set_node_property_inner(node_id, key, value, Some(&mut before_images)) {
            Ok(()) => match self.commit_tx(tx_id) {
                Ok(()) => Ok(()),
                Err(err) => {
                    self.rollback_pages(&before_images)?;
                    Err(err)
                }
            },
            Err(err) => {
                self.rollback_pages(&before_images)?;
                Err(err)
            }
        }
    }

    pub(crate) fn set_node_property_inner(
        &mut self,
        node_id: NodeId,
        key: &str,
        value: &Value,
        mut before_images: Option<&mut Vec<BeforeImage>>,
    ) -> Result<(), DbError> {

        let (page_id, slot_id) = unpack_record_id(node_id);
        if slot_id == u16::MAX {
            return Err(DbError::ReadError);
        }

        let mut node = self.get_node(node_id)?;
        let key_hash = crate::value::hash_key(key);
        let (value_type, value_inline) = value.to_inline_bytes();

        let long_value_offset = if value_type == value::LONG_STRING {
            if let Value::String(s) = value {
                self.write_overflow_string(s.as_bytes(), &mut before_images)? as u64
            } else {
                0
            }
        } else {
            0
        };

        let existing = node.properties.iter_mut().find(|p| p.key_hash == key_hash);
        if let Some(entry) = existing {
            entry.value_type = value_type;
            entry.value_inline = value_inline;
            entry.long_value_offset = long_value_offset;
        } else {
            node.properties.push(PropertyEntry {
                key_hash,
                value_type,
                value_inline,
                long_value_offset,
            });
        }

        let mut record_buf = vec![0u8; node.encoded_size()];
        node.to_bytes(&mut record_buf)?;

        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        layout::update_record(page_buf, slot_id, &record_buf)?;

        Ok(())
    }

    /// Gets a property value from a node by key.
    /// Reads long strings from overflow pages when needed.
    pub fn get_node_property(&mut self, node_id: NodeId, key: &str) -> Result<Value, DbError> {
        let node = self.get_node(node_id)?;
        let key_hash = crate::value::hash_key(key);

        let entry = node
            .properties
            .iter()
            .find(|p| p.key_hash == key_hash)
            .ok_or(DbError::ReadError)?;

        if entry.value_type == value::LONG_STRING && entry.long_value_offset != 0 {
            let data = OverflowStore::read_string(&mut self.pager, entry.long_value_offset as u32)?;
            let s = String::from_utf8(data).map_err(|_| DbError::ReadError)?;
            return Ok(Value::String(s));
        }

        Ok(Value::from_bytes(entry.value_type, entry.value_inline))
    }

    /// Sets a property on an edge. Updates or appends the property entry.
    /// Long strings (> 15 bytes) are stored in overflow pages.
    pub fn set_edge_property(
        &mut self,
        edge_id: EdgeId,
        key: &str,
        value: &Value,
    ) -> Result<(), DbError> {
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();

        match self.set_edge_property_inner(edge_id, key, value, Some(&mut before_images)) {
            Ok(()) => match self.commit_tx(tx_id) {
                Ok(()) => Ok(()),
                Err(err) => {
                    self.rollback_pages(&before_images)?;
                    Err(err)
                }
            },
            Err(err) => {
                self.rollback_pages(&before_images)?;
                Err(err)
            }
        }
    }

    pub(crate) fn set_edge_property_inner(
        &mut self,
        edge_id: EdgeId,
        key: &str,
        value: &Value,
        mut before_images: Option<&mut Vec<BeforeImage>>,
    ) -> Result<(), DbError> {

        let (page_id, slot_id) = unpack_record_id(edge_id);
        if slot_id == u16::MAX {
            return Err(DbError::ReadError);
        }

        let mut edge = self.get_edge(edge_id)?;
        let key_hash = crate::value::hash_key(key);
        let (value_type, value_inline) = value.to_inline_bytes();

        let long_value_offset = if value_type == value::LONG_STRING {
            if let Value::String(s) = value {
                self.write_overflow_string(s.as_bytes(), &mut before_images)? as u64
            } else {
                0
            }
        } else {
            0
        };

        let existing = edge.properties.iter_mut().find(|p| p.key_hash == key_hash);
        if let Some(entry) = existing {
            entry.value_type = value_type;
            entry.value_inline = value_inline;
            entry.long_value_offset = long_value_offset;
        } else {
            edge.properties.push(PropertyEntry {
                key_hash,
                value_type,
                value_inline,
                long_value_offset,
            });
        }

        let mut record_buf = vec![0u8; edge.encoded_size()];
        edge.to_bytes(&mut record_buf)?;

        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        layout::update_record(page_buf, slot_id, &record_buf)?;

        Ok(())
    }

    /// Gets a property value from an edge by key.
    /// Reads long strings from overflow pages when needed.
    pub fn get_edge_property(&mut self, edge_id: EdgeId, key: &str) -> Result<Value, DbError> {
        let edge = self.get_edge(edge_id)?;
        let key_hash = crate::value::hash_key(key);

        let entry = edge
            .properties
            .iter()
            .find(|p| p.key_hash == key_hash)
            .ok_or(DbError::ReadError)?;

        if entry.value_type == value::LONG_STRING && entry.long_value_offset != 0 {
            let data = OverflowStore::read_string(&mut self.pager, entry.long_value_offset as u32)?;
            let s = String::from_utf8(data).map_err(|_| DbError::ReadError)?;
            return Ok(Value::String(s));
        }

        Ok(Value::from_bytes(entry.value_type, entry.value_inline))
    }

    /// Finds an existing DataNode page with free space, or allocates a new one.
    fn find_or_alloc_data_node_page(
        &mut self,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
    ) -> Result<u32, DbError> {
        let root_page = {
            let meta_page = self.pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.root_data_page
        };

        if root_page != 0 {
            let page_buf = self.pager.get_page(root_page)?;
            if layout::get_free_space(page_buf) > 0 {
                return Ok(root_page);
            }
        }

        let new_page = self.pager.allocate_page()?;
        Self::capture_allocated_page(&mut self.pager, before_images, new_page)?;
        let page_buf = self.pager.get_page_mut(new_page)?;
        layout::init_regular_page(page_buf, PageType::DataNode);

        self.update_meta_root_data_page(new_page, before_images)?;

        Ok(new_page)
    }

    /// Finds an existing DataEdge page with free space, or allocates a new one.
    fn find_or_alloc_data_edge_page(
        &mut self,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
    ) -> Result<u32, DbError> {
        let root_page = {
            let meta_page = self.pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.root_edge_page
        };

        if root_page != 0 {
            let page_buf = self.pager.get_page(root_page)?;
            if layout::get_free_space(page_buf) > 0 {
                return Ok(root_page);
            }
        }

        let new_page = self.pager.allocate_page()?;
        Self::capture_allocated_page(&mut self.pager, before_images, new_page)?;
        let page_buf = self.pager.get_page_mut(new_page)?;
        layout::init_regular_page(page_buf, PageType::DataEdge);

        self.update_meta_root_edge_page(new_page, before_images)?;

        Ok(new_page)
    }

    /// Updates the node count in the meta header.
    fn update_meta_node_count(
        &mut self,
        count: u64,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
    ) -> Result<(), DbError> {
        Self::capture_before_image(&mut self.pager, before_images, META_PAGE_ID)?;
        let meta_page = self.pager.get_page_mut(META_PAGE_ID)?;
        let mut meta = layout::read_meta_header(meta_page);
        meta.node_count = count;
        layout::write_meta_header(meta_page, &meta);
        Ok(())
    }

    /// Updates the edge count in the meta header.
    fn update_meta_edge_count(
        &mut self,
        count: u64,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
    ) -> Result<(), DbError> {
        Self::capture_before_image(&mut self.pager, before_images, META_PAGE_ID)?;
        let meta_page = self.pager.get_page_mut(META_PAGE_ID)?;
        let mut meta = layout::read_meta_header(meta_page);
        meta.edge_count = count;
        layout::write_meta_header(meta_page, &meta);
        Ok(())
    }

    /// Updates the root_data_page pointer in the meta header.
    fn update_meta_root_data_page(
        &mut self,
        page_id: u32,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
    ) -> Result<(), DbError> {
        Self::capture_before_image(&mut self.pager, before_images, META_PAGE_ID)?;
        let meta_page = self.pager.get_page_mut(META_PAGE_ID)?;
        let mut meta = layout::read_meta_header(meta_page);
        meta.root_data_page = page_id;
        layout::write_meta_header(meta_page, &meta);
        Ok(())
    }

    /// Updates the root_edge_page pointer in the meta header.
    fn update_meta_root_edge_page(
        &mut self,
        page_id: u32,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
    ) -> Result<(), DbError> {
        Self::capture_before_image(&mut self.pager, before_images, META_PAGE_ID)?;
        let meta_page = self.pager.get_page_mut(META_PAGE_ID)?;
        let mut meta = layout::read_meta_header(meta_page);
        meta.root_edge_page = page_id;
        layout::write_meta_header(meta_page, &meta);
        Ok(())
    }

    /// Returns a new unique transaction ID.
    pub(crate) fn next_tx_id(&self) -> TxId {
        self.next_tx_id.fetch_add(1, Ordering::SeqCst)
    }

    fn capture_before_image(
        pager: &mut Pager,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
        page_id: u32,
    ) -> Result<(), DbError> {
        Self::capture_page_image(pager, before_images, page_id, false)
    }

    fn write_overflow_string(
        &mut self,
        data: &[u8],
        before_images: &mut Option<&mut Vec<BeforeImage>>,
    ) -> Result<u32, DbError> {
        let page_id = self.pager.allocate_page()?;
        Self::capture_allocated_page(&mut self.pager, before_images, page_id)?;
        OverflowStore::write_string_to_page(&mut self.pager, page_id, data)?;
        Ok(page_id)
    }

    fn capture_allocated_page(
        pager: &mut Pager,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
        page_id: u32,
    ) -> Result<(), DbError> {
        Self::capture_page_image(pager, before_images, page_id, true)
    }

    fn capture_page_image(
        pager: &mut Pager,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
        page_id: u32,
        newly_allocated: bool,
    ) -> Result<(), DbError> {
        let Some(images) = before_images.as_deref_mut() else {
            return Ok(());
        };

        if images.iter().any(|image| image.page_id == page_id) {
            return Ok(());
        }

        let page = pager.read_page(page_id)?;
        images.push(BeforeImage {
            page_id,
            bytes: page,
            newly_allocated,
        });
        Ok(())
    }

    pub(crate) fn rollback_pages(&mut self, before_images: &[BeforeImage]) -> Result<(), DbError> {
        for image in before_images.iter().rev() {
            self.pager.restore_page(image.page_id, &image.bytes)?;
            self.pager.mark_clean(image.page_id)?;
            if image.newly_allocated {
                self.pager.free_page(image.page_id)?;
            }
        }
        Ok(())
    }

    /// Sets the automatic checkpoint interval in committed transactions.
    ///
    /// `0` disables automatic checkpointing.
    pub fn set_auto_checkpoint_interval(&mut self, interval: u64) {
        self.auto_checkpoint_interval = interval;
    }

    /// Begins a new explicit transaction.
    pub fn begin(&mut self) -> Result<Transaction<'_>, DbError> {
        let tx_id = self.next_tx_id();
        Transaction::new(self, tx_id)
    }

    /// Commits a transaction by writing dirty page images to the WAL,
    /// syncing, and stamping page LSNs.
    pub(crate) fn commit_tx(&mut self, tx_id: TxId) -> Result<(), DbError> {
        let dirty_pages = self.pager.dirty_page_ids();

        let begin_lsn = self.pager.next_lsn();
        let mut entries = Vec::with_capacity(dirty_pages.len() + 2);
        entries.push(WalEntry::Begin {
            tx_id,
            lsn: begin_lsn,
        });

        for page_id in &dirty_pages {
            let page_lsn = self.pager.next_lsn();
            self.pager.stamp_page_lsn(*page_id, page_lsn)?;
            let page = *self.pager.get_page(*page_id)?;
            entries.push(WalEntry::PageImage {
                tx_id,
                lsn: page_lsn,
                page_id: *page_id,
                page_lsn,
                bytes: Box::new(page),
            });
        }

        let commit_lsn = self.pager.next_lsn();
        entries.push(WalEntry::Commit {
            tx_id,
            lsn: commit_lsn,
        });

        self.wal.append_batch(&entries)?;
        self.wal.sync()?;

        for page_id in &dirty_pages {
            self.pager.mark_spilled(*page_id)?;
        }

        self.commits_since_checkpoint += 1;
        if self.auto_checkpoint_interval > 0
            && self.commits_since_checkpoint >= self.auto_checkpoint_interval
        {
            self.checkpoint()?;
        }

        Ok(())
    }

    /// Writes a checkpoint: flushes all dirty pages to disk and truncates the WAL.
    pub fn checkpoint(&mut self) -> Result<(), DbError> {
        self.pager.flush_file()?;
        self.pager.sync_file()?;
        self.wal.checkpoint()?;
        self.commits_since_checkpoint = 0;
        Ok(())
    }

    pub fn close(mut self) {
        let _ = self.pager.sync_all();
    }
}
