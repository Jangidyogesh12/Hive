use crate::errors::DbError;
use crate::storage::btree::BTree;
use crate::storage::label_store::LabelStore;
use crate::storage::overflow_store::OverflowStore;
use crate::storage::page::format::{
    META_PAGE_ID, MetaHeader, PAGE_SIZE, PageType, SLOT_ENTRY_SIZE,
};
use crate::storage::page::layout;
use crate::storage::page::record::{EdgeRecord, NodeRecord, PropertyEntry};
use crate::storage::pager::Pager;
use crate::storage::property_key_store::PropertyKeyStore;
use crate::transaction::Transaction;
use crate::types::{EdgeId, NIL_ID, NodeId, pack_record_id, unpack_record_id};
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
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();

        match self.register_label_inner(name, Some(&mut before_images)) {
            Ok(label_id) => match self.commit_tx(tx_id) {
                Ok(()) => Ok(label_id),
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

    pub(crate) fn register_label_inner(
        &mut self,
        name: &str,
        mut before_images: Option<&mut Vec<BeforeImage>>,
    ) -> Result<u32, DbError> {
        if let Some(existing_id) = LabelStore::find_label(&mut self.pager, name)? {
            return Ok(existing_id);
        }

        let label_id = {
            let meta_page = self.pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.label_count as u32 + 1
        };

        let entry_buf = LabelStore::encode_label_entry(label_id, name)?;
        let page_id = self.find_or_alloc_page(
            &mut before_images,
            PageType::LabelData,
            entry_buf.len() + SLOT_ENTRY_SIZE,
        )?;

        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        layout::insert_record(page_buf, &entry_buf)?;

        self.update_meta_header(&mut before_images, |meta| {
            meta.label_count = label_id as u64;
        })?;

        Ok(label_id)
    }

    /// Returns the label name for a given ID.
    pub fn get_label_name(&mut self, label_id: u32) -> Result<Option<String>, DbError> {
        LabelStore::get_label_name(&mut self.pager, label_id)
    }

    /// Looks up the label id for a given name, or returns `None` if not found.
    pub fn find_label(&mut self, name: &str) -> Result<Option<u32>, DbError> {
        LabelStore::find_label(&mut self.pager, name)
    }

    /// Registers a property-key name and returns its numeric ID.
    pub fn register_property_key(&mut self, name: &str) -> Result<u32, DbError> {
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();

        match self.register_property_key_inner(name, Some(&mut before_images)) {
            Ok(key_id) => match self.commit_tx(tx_id) {
                Ok(()) => Ok(key_id),
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

    pub(crate) fn register_property_key_inner(
        &mut self,
        name: &str,
        mut before_images: Option<&mut Vec<BeforeImage>>,
    ) -> Result<u32, DbError> {
        if let Some(existing_id) = PropertyKeyStore::find_property_key(&mut self.pager, name)? {
            return Ok(existing_id);
        }

        let key_id = {
            let meta_page = self.pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.property_count as u32 + 1
        };

        let entry_buf = PropertyKeyStore::encode_property_key_entry(key_id, name)?;
        let page_id = self.find_or_alloc_page(
            &mut before_images,
            PageType::PropertyKeyData,
            entry_buf.len() + SLOT_ENTRY_SIZE,
        )?;

        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        layout::insert_record(page_buf, &entry_buf)?;

        self.update_meta_header(&mut before_images, |meta| {
            meta.property_count = key_id as u64;
        })?;

        Ok(key_id)
    }

    /// Returns the property-key name for a given `key_id`, or `None` if not found.
    pub fn get_property_key_name(&mut self, key_id: u32) -> Result<Option<String>, DbError> {
        PropertyKeyStore::get_property_key_name(&mut self.pager, key_id)
    }

    /// Looks up the `key_id` for a given property name, or returns `None` if not found.
    pub fn find_property_key(&mut self, name: &str) -> Result<Option<u32>, DbError> {
        PropertyKeyStore::find_property_key(&mut self.pager, name)
    }

    /// Creates a new empty B-tree index, persists its root page id, and commits.
    /// Returns the root page id of the new tree.
    pub fn create_btree(&mut self) -> Result<u32, DbError> {
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();

        let root = match self.pager.allocate_page() {
            Ok(root) => root,
            Err(err) => {
                self.rollback_pages(&before_images)?;
                return Err(err);
            }
        };
        if let Err(err) =
            Self::capture_allocated_page(&mut self.pager, &mut Some(&mut before_images), root)
        {
            self.rollback_pages(&before_images)?;
            return Err(err);
        }
        {
            let buf = self.pager.get_page_mut(root)?;
            crate::storage::btree::page::init_leaf_page(buf);
        }

        if let Err(err) = self.update_meta_header(&mut Some(&mut before_images), |meta| {
            meta.root_index_page = root;
        }) {
            self.rollback_pages(&before_images)?;
            return Err(err);
        }

        match self.commit_tx(tx_id) {
            Ok(()) => Ok(root),
            Err(err) => {
                self.rollback_pages(&before_images)?;
                Err(err)
            }
        }
    }

    /// Opens an existing B-tree by its root page id.
    pub fn open_btree(&mut self, root_page_id: u32) -> BTree<'_> {
        BTree::open(&mut self.pager, root_page_id)
    }

    /// Creates a node label index and returns the data B-tree root page id.
    pub fn create_node_label_index(&mut self, label: &str) -> Result<u32, DbError> {
        let tx_id = self.next_tx_id();
        let mut tx = Transaction::new(self, tx_id)?;
        let label_id = tx.register_label(label)?;
        let root = tx.create_index(
            crate::storage::index_catalog::EntityKind::NodeLabel,
            label_id,
            0,
        )?;
        tx.commit()?;
        Ok(root)
    }

    /// Creates an edge type index and returns the data B-tree root page id.
    pub fn create_edge_type_index(&mut self, rel_type: &str) -> Result<u32, DbError> {
        let tx_id = self.next_tx_id();
        let mut tx = Transaction::new(self, tx_id)?;
        let type_id = tx.register_label(rel_type)?;
        let root = tx.create_index(
            crate::storage::index_catalog::EntityKind::EdgeType,
            type_id,
            0,
        )?;
        tx.commit()?;
        Ok(root)
    }

    /// Creates a node property index.
    /// If `label` is `Some`, the index covers only nodes with that label.
    /// If `label` is `None`, the index covers all nodes with the property.
    pub fn create_node_property_index(
        &mut self,
        label: Option<&str>,
        key: &str,
    ) -> Result<u32, DbError> {
        let tx_id = self.next_tx_id();
        let mut tx = Transaction::new(self, tx_id)?;
        let label_id = match label {
            Some(l) => tx.register_label(l)?,
            None => 0,
        };
        let key_id = tx.register_property_key(key)?;
        let root = tx.create_index(
            crate::storage::index_catalog::EntityKind::NodeProperty,
            label_id,
            key_id,
        )?;
        tx.commit()?;
        Ok(root)
    }

    /// Creates an edge property index.
    /// If `rel_type` is `Some`, the index covers only edges with that type.
    /// If `rel_type` is `None`, the index covers all edges with the property.
    pub fn create_edge_property_index(
        &mut self,
        rel_type: Option<&str>,
        key: &str,
    ) -> Result<u32, DbError> {
        let tx_id = self.next_tx_id();
        let mut tx = Transaction::new(self, tx_id)?;
        let label_id = match rel_type {
            Some(t) => tx.register_label(t)?,
            None => 0,
        };
        let key_id = tx.register_property_key(key)?;
        let root = tx.create_index(
            crate::storage::index_catalog::EntityKind::EdgeProperty,
            label_id,
            key_id,
        )?;
        tx.commit()?;
        Ok(root)
    }

    /// Parses, plans, and executes a Cypher-like query as one database operation.
    pub fn execute(&mut self, query: &str) -> Result<crate::query::result::QueryResult, DbError> {
        let statement = crate::query::parser::parse(query)
            .map_err(|err| DbError::QueryError(err.to_string()))?;
        let plan = crate::query::planner::plan(statement)?;
        crate::query::executor::execute(&plan, self)
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

        let record = NodeRecord::new(node_id_counter);
        let page_id = self.find_or_alloc_page(
            &mut before_images,
            PageType::DataNode,
            record.encoded_size() + SLOT_ENTRY_SIZE,
        )?;

        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        let mut record = record;
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

    /// Scans every live node record in DataNode pages.
    pub fn scan_nodes(&mut self) -> Result<Vec<(NodeId, NodeRecord)>, DbError> {
        let mut out = Vec::new();
        let page_count = self.pager.page_count()? as u32;
        for page_id in 1..page_count {
            let page_buf = self.pager.get_page(page_id)?;
            let header = layout::read_page_header(page_buf);
            if header.page_type != PageType::DataNode {
                continue;
            }
            for slot_id in 0..header.slot_count {
                if let Some(bytes) = layout::read_record_bytes(page_buf, slot_id) {
                    out.push((
                        pack_record_id(page_id, slot_id),
                        NodeRecord::from_bytes(bytes)?,
                    ));
                }
            }
        }
        Ok(out)
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

        match self.create_edge_with_label_inner(src_id, dst_id, label_id, Some(&mut before_images))
        {
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

        let mut edge = EdgeRecord::new(edge_id_counter);
        edge.src = src_id;
        edge.dst = dst_id;
        edge.label_id = label_id;

        let (src_page_id, src_slot_id) = unpack_record_id(src_id);
        let (dst_page_id, dst_slot_id) = unpack_record_id(dst_id);

        let mut src_node = self.get_node(src_id).unwrap();
        let mut dst_node = self.get_node(dst_id).unwrap();

        edge.next_out_edge = src_node.first_out_edge;
        edge.next_in_edge = dst_node.first_in_edge;

        let page_id = self.find_or_alloc_page(
            &mut before_images,
            PageType::DataEdge,
            edge.encoded_size() + SLOT_ENTRY_SIZE,
        )?;

        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;

        let mut record_buf = vec![0u8; edge.encoded_size()];
        edge.to_bytes(&mut record_buf)?;
        let slot = layout::insert_record(page_buf, &record_buf)?;

        self.update_meta_edge_count(edge_id_counter, &mut before_images)?;

        let new_edge_id = pack_record_id(page_id, slot.0);

        // Update src node: first_out_edge -> new edge
        Self::capture_before_image(&mut self.pager, &mut before_images, src_page_id)?;
        src_node.first_out_edge = new_edge_id;
        let mut src_buf = vec![0u8; src_node.encoded_size()];
        src_node.to_bytes(&mut src_buf)?;
        let page_buf = self.pager.get_page_mut(src_page_id)?;
        layout::update_record(page_buf, src_slot_id, &src_buf)?;

        // Update dst node : first_in_edge -> new_edg
        Self::capture_before_image(&mut self.pager, &mut before_images, dst_page_id)?;
        dst_node.first_in_edge = new_edge_id;
        let mut dst_buf = vec![0u8; dst_node.encoded_size()];
        dst_node.to_bytes(&mut dst_buf)?;
        let page_buf = self.pager.get_page_mut(dst_page_id)?;
        layout::update_record(page_buf, dst_slot_id, &dst_buf)?;

        Ok(new_edge_id)
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

    /// Scans every live edge record in DataEdge pages.
    pub fn scan_edges(&mut self) -> Result<Vec<(EdgeId, EdgeRecord)>, DbError> {
        let mut out = Vec::new();
        let page_count = self.pager.page_count()? as u32;
        for page_id in 1..page_count {
            let page_buf = self.pager.get_page(page_id)?;
            let header = layout::read_page_header(page_buf);
            if header.page_type != PageType::DataEdge {
                continue;
            }
            for slot_id in 0..header.slot_count {
                if let Some(bytes) = layout::read_record_bytes(page_buf, slot_id) {
                    out.push((
                        pack_record_id(page_id, slot_id),
                        EdgeRecord::from_bytes(bytes)?,
                    ));
                }
            }
        }
        Ok(out)
    }

    /// Walks the adjacency chain from a node and returns its connected edges.
    pub fn get_edges_from_node(
        &mut self,
        node_id: NodeId,
        outgoing: bool,
    ) -> Result<Vec<(EdgeId, EdgeRecord)>, DbError> {
        let node = self.get_node(node_id)?;
        let mut out = Vec::new();
        let mut current = if outgoing {
            node.first_out_edge
        } else {
            node.first_in_edge
        };
        while current != NIL_ID {
            let (page_id, slot_id) = unpack_record_id(current);
            let page_buf = self.pager.get_page(page_id)?;
            if let Some(bytes) = layout::read_record_bytes(page_buf, slot_id) {
                let edge = EdgeRecord::from_bytes(bytes)?;
                let next = if outgoing {
                    edge.next_out_edge
                } else {
                    edge.next_in_edge
                };
                out.push((current, edge));
                current = next;
            } else {
                break;
            }
        }
        Ok(out)
    }

    /// Returns `true` if the node has any incident edges.
    pub fn node_has_edges(&mut self, node_id: NodeId) -> Result<bool, DbError> {
        let node = self.get_node(node_id)?;
        Ok(node.first_out_edge != NIL_ID || node.first_in_edge != NIL_ID)
    }

    /// Deletes an edge by its packed EdgeId.  Wraps in an auto-committed transaction.
    pub fn delete_edge(&mut self, edge_id: EdgeId) -> Result<(), DbError> {
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();
        match self.delete_edge_inner(edge_id, Some(&mut before_images)) {
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

    /// Inner implementation of edge deletion.  Optionally captures before-images for rollback.
    pub(crate) fn delete_edge_inner(
        &mut self,
        edge_id: EdgeId,
        mut before_images: Option<&mut Vec<BeforeImage>>,
    ) -> Result<(), DbError> {
        let edge = self.get_edge(edge_id)?;

        // --- Unlink from source's outgoing chain ---
        let (src_page_id, src_slot_id) = unpack_record_id(edge.src);
        let mut src_node = self.get_node(edge.src)?;

        if src_node.first_out_edge == edge_id {
            // Edge is the head of the outgoing chain — move head to next
            Self::capture_before_image(&mut self.pager, &mut before_images, src_page_id)?;
            src_node.first_out_edge = edge.next_out_edge;
            let mut src_buf = vec![0u8; src_node.encoded_size()];
            src_node.to_bytes(&mut src_buf)?;
            let page_buf = self.pager.get_page_mut(src_page_id)?;
            layout::update_record(page_buf, src_slot_id, &src_buf)?;
        } else {
            // Walk chain to find predecessor
            let mut current = src_node.first_out_edge;
            while current != NIL_ID {
                let cur_edge = self.get_edge(current)?;
                if cur_edge.next_out_edge == edge_id {
                    // Found predecessor — re-link it to skip the deleted edge
                    let (pred_page_id, pred_slot_id) = unpack_record_id(current);
                    Self::capture_before_image(&mut self.pager, &mut before_images, pred_page_id)?;
                    let mut updated = cur_edge;
                    updated.next_out_edge = edge.next_out_edge;
                    let mut pred_buf = vec![0u8; updated.encoded_size()];
                    updated.to_bytes(&mut pred_buf)?;
                    let page_buf = self.pager.get_page_mut(pred_page_id)?;
                    layout::update_record(page_buf, pred_slot_id, &pred_buf)?;
                    break;
                }
                current = cur_edge.next_out_edge;
            }
        }

        // --- Unlink from destination's incoming chain ---
        let (dst_page_id, dst_slot_id) = unpack_record_id(edge.dst);
        let mut dst_node = self.get_node(edge.dst)?;

        if dst_node.first_in_edge == edge_id {
            // Edge is the head of the incoming chain — move head to next
            Self::capture_before_image(&mut self.pager, &mut before_images, dst_page_id)?;
            dst_node.first_in_edge = edge.next_in_edge;
            let mut dst_buf = vec![0u8; dst_node.encoded_size()];
            dst_node.to_bytes(&mut dst_buf)?;
            let page_buf = self.pager.get_page_mut(dst_page_id)?;
            layout::update_record(page_buf, dst_slot_id, &dst_buf)?;
        } else {
            // Walk chain to find predecessor
            let mut current = dst_node.first_in_edge;
            while current != NIL_ID {
                let cur_edge = self.get_edge(current)?;
                if cur_edge.next_in_edge == edge_id {
                    // Found predecessor — re-link it to skip the deleted edge
                    let (pred_page_id, pred_slot_id) = unpack_record_id(current);
                    Self::capture_before_image(&mut self.pager, &mut before_images, pred_page_id)?;
                    let mut updated = cur_edge;
                    updated.next_in_edge = edge.next_in_edge;
                    let mut pred_buf = vec![0u8; updated.encoded_size()];
                    updated.to_bytes(&mut pred_buf)?;
                    let page_buf = self.pager.get_page_mut(pred_page_id)?;
                    layout::update_record(page_buf, pred_slot_id, &pred_buf)?;
                    break;
                }
                current = cur_edge.next_in_edge;
            }
        }

        // --- Finally, mark the edge record as dead ---
        let (page_id, slot_id) = unpack_record_id(edge_id);
        if slot_id == u16::MAX {
            return Err(DbError::ReadError);
        }
        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        layout::delete_record(page_buf, slot_id)
    }

    /// Deletes a node by its packed NodeId.  Fails if the node has incident edges.
    pub fn delete_node(&mut self, node_id: NodeId) -> Result<(), DbError> {
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();
        match self.delete_node_inner(node_id, Some(&mut before_images)) {
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

    /// Inner implementation of node deletion.  Optionally captures before-images for rollback.
    pub(crate) fn delete_node_inner(
        &mut self,
        node_id: NodeId,
        mut before_images: Option<&mut Vec<BeforeImage>>,
    ) -> Result<(), DbError> {
        if self.node_has_edges(node_id)? {
            return Err(DbError::QueryError(
                "cannot delete node with incident edges without DETACH DELETE".to_string(),
            ));
        }
        let (page_id, slot_id) = unpack_record_id(node_id);
        if slot_id == u16::MAX {
            return Err(DbError::ReadError);
        }
        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        layout::delete_record(page_buf, slot_id)
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
        let key_id = self.register_property_key_inner(key, before_images.as_deref_mut())?;
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

        let existing = node.properties.iter_mut().find(|p| p.key_id == key_id);
        if let Some(entry) = existing {
            entry.value_type = value_type;
            entry.value_inline = value_inline;
            entry.long_value_offset = long_value_offset;
        } else {
            node.properties.push(PropertyEntry {
                key_id,
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
        let key_id = self.find_property_key(key)?.ok_or(DbError::ReadError)?;

        let entry = node
            .properties
            .iter()
            .find(|p| p.key_id == key_id)
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
        let key_id = self.register_property_key_inner(key, before_images.as_deref_mut())?;
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

        let existing = edge.properties.iter_mut().find(|p| p.key_id == key_id);
        if let Some(entry) = existing {
            entry.value_type = value_type;
            entry.value_inline = value_inline;
            entry.long_value_offset = long_value_offset;
        } else {
            edge.properties.push(PropertyEntry {
                key_id,
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
        let key_id = self.find_property_key(key)?.ok_or(DbError::ReadError)?;

        let entry = edge
            .properties
            .iter()
            .find(|p| p.key_id == key_id)
            .ok_or(DbError::ReadError)?;

        if entry.value_type == value::LONG_STRING && entry.long_value_offset != 0 {
            let data = OverflowStore::read_string(&mut self.pager, entry.long_value_offset as u32)?;
            let s = String::from_utf8(data).map_err(|_| DbError::ReadError)?;
            return Ok(Value::String(s));
        }

        Ok(Value::from_bytes(entry.value_type, entry.value_inline))
    }

    /// Lists all properties on a node as (key_name, value) pairs.
    pub fn list_node_properties(
        &mut self,
        node_id: NodeId,
    ) -> Result<Vec<(String, Value)>, DbError> {
        let node = self.get_node(node_id)?;
        let mut out = Vec::with_capacity(node.properties.len());
        for entry in &node.properties {
            let key_name = self
                .get_property_key_name(entry.key_id)?
                .unwrap_or_else(|| format!("key_{}", entry.key_id));
            if entry.value_type == value::LONG_STRING && entry.long_value_offset != 0 {
                let data =
                    OverflowStore::read_string(&mut self.pager, entry.long_value_offset as u32)?;
                let s = String::from_utf8(data).map_err(|_| DbError::ReadError)?;
                out.push((key_name, Value::String(s)));
            } else {
                out.push((
                    key_name,
                    Value::from_bytes(entry.value_type, entry.value_inline),
                ));
            }
        }
        Ok(out)
    }

    /// Lists all properties on an edge as (key_name, value) pairs.
    pub fn list_edge_properties(
        &mut self,
        edge_id: EdgeId,
    ) -> Result<Vec<(String, Value)>, DbError> {
        let edge = self.get_edge(edge_id)?;
        let mut out = Vec::with_capacity(edge.properties.len());
        for entry in &edge.properties {
            let key_name = self
                .get_property_key_name(entry.key_id)?
                .unwrap_or_else(|| format!("key_{}", entry.key_id));
            if entry.value_type == value::LONG_STRING && entry.long_value_offset != 0 {
                let data =
                    OverflowStore::read_string(&mut self.pager, entry.long_value_offset as u32)?;
                let s = String::from_utf8(data).map_err(|_| DbError::ReadError)?;
                out.push((key_name, Value::String(s)));
            } else {
                out.push((
                    key_name,
                    Value::from_bytes(entry.value_type, entry.value_inline),
                ));
            }
        }
        Ok(out)
    }

    /// Finds an existing DataEdge page with free space, or allocates a new one.
    fn find_or_alloc_page(
        &mut self,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
        page_type: PageType,
        required_space: usize,
    ) -> Result<u32, DbError> {
        let root_page = {
            let meta_page = self.pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            Self::root_page_id_for_type(meta, page_type)?
        };

        if root_page != 0 {
            let page_buf = self.pager.get_page(root_page)?;
            if layout::get_free_space(page_buf) >= required_space {
                return Ok(root_page);
            }
        }

        let new_page = self.pager.allocate_page()?;
        Self::capture_allocated_page(&mut self.pager, before_images, new_page)?;
        let page_buf = self.pager.get_page_mut(new_page)?;
        layout::init_regular_page(page_buf, page_type);

        match page_type {
            PageType::DataEdge => {
                self.update_meta_header(before_images, |meta| {
                    meta.root_edge_page = new_page;
                })?;
            }
            PageType::DataNode => {
                self.update_meta_header(before_images, |meta| {
                    meta.root_node_page = new_page;
                })?;
            }
            PageType::LabelData => {
                self.update_meta_header(before_images, |meta| {
                    meta.root_label_page = new_page;
                })?;
            }
            PageType::PropertyKeyData => {
                self.update_meta_header(before_images, |meta| {
                    meta.root_string_page = new_page;
                })?;
            }
            _ => {}
        }

        Ok(new_page)
    }

    // Gets the page_id of the root page of particular type (Node Page, Edge Page, Label Page)
    fn root_page_id_for_type(meta: MetaHeader, page_type: PageType) -> Result<u32, DbError> {
        match page_type {
            PageType::DataNode => Ok(meta.root_node_page),
            PageType::DataEdge => Ok(meta.root_edge_page),
            PageType::LabelData => Ok(meta.root_label_page),
            PageType::PropertyKeyData => Ok(meta.root_string_page),
            _ => Err(DbError::WriteError),
        }
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

    /// Updates the meta header after capturing its before imager.
    pub(crate) fn update_meta_header(
        &mut self,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
        update: impl FnOnce(&mut MetaHeader),
    ) -> Result<(), DbError> {
        Self::capture_before_image(&mut self.pager, before_images, META_PAGE_ID)?;
        let meta_page = self.pager.get_page_mut(META_PAGE_ID)?;
        let mut meta = layout::read_meta_header(meta_page);
        update(&mut meta);
        layout::write_meta_header(meta_page, &meta);
        Ok(())
    }

    /// Returns a new unique transaction ID.
    pub(crate) fn next_tx_id(&self) -> TxId {
        self.next_tx_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Captures the current page image for rollback before modifying it.
    pub(crate) fn capture_before_image(
        pager: &mut Pager,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
        page_id: u32,
    ) -> Result<(), DbError> {
        Self::capture_page_image(pager, before_images, page_id, false)
    }

    /// Writes a long string to an overflow page and returns the page ID.
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

    /// Captures a newly allocated page so it can be freed on rollback.
    pub(crate) fn capture_allocated_page(
        pager: &mut Pager,
        before_images: &mut Option<&mut Vec<BeforeImage>>,
        page_id: u32,
    ) -> Result<(), DbError> {
        Self::capture_page_image(pager, before_images, page_id, true)
    }

    /// Core before-image capture: copies the current page bytes if not already captured.
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

    /// Restores all pages to their state before the transaction began.
    /// Newly allocated pages are freed; existing pages are overwritten.
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

    /// Commits a read-only transaction by marking pages clean without WAL work.
    ///
    /// Used for queries that perform no mutations (no CREATE, MERGE, SET, DELETE).
    /// Label and property-key registrations that occur during read-only queries
    /// are idempotent and safe to leave on disk without WAL protection.
    pub(crate) fn commit_readonly(&mut self) -> Result<(), DbError> {
        for page_id in self.pager.dirty_page_ids() {
            self.pager.mark_spilled(page_id)?;
        }
        Ok(())
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

    /// Closes the database, flushing all pending writes to disk.
    pub fn close(mut self) {
        let _ = self.pager.sync_all();
    }
}
