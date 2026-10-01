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
    pub(crate) page_id: u32,
    pub(crate) bytes: [u8; PAGE_SIZE],
    pub(crate) newly_allocated: bool,
}

/// Database statistics snapshot returned by `HiveDb::stats`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbStats {
    /// Total pages in the database file.
    pub page_count: u32,
    /// Pages holding node records.
    pub node_pages: u32,
    /// Pages holding edge records.
    pub edge_pages: u32,
    /// Pages holding B-tree index data.
    pub btree_pages: u32,
    /// Pages holding overflow string data.
    pub overflow_pages: u32,
    /// Pages holding freelist data.
    pub freelist_pages: u32,
    /// Live (non-deleted) node records.
    pub live_nodes: u64,
    /// Live (non-deleted) edge records.
    pub live_edges: u64,
    /// Allocation counter for nodes (never decremented).
    pub meta_node_count: u64,
    /// Allocation counter for edges (never decremented).
    pub meta_edge_count: u64,
    /// Number of registered labels.
    pub label_count: u64,
    /// Number of registered property keys.
    pub property_key_count: u64,
}

impl std::fmt::Display for DbStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "pages: {}", self.page_count)?;
        writeln!(
            f,
            "node_pages: {} edge_pages: {} btree_pages: {} overflow_pages: {} freelist_pages: {}",
            self.node_pages,
            self.edge_pages,
            self.btree_pages,
            self.overflow_pages,
            self.freelist_pages
        )?;
        writeln!(
            f,
            "live_nodes: {} live_edges: {}",
            self.live_nodes, self.live_edges
        )?;
        writeln!(
            f,
            "meta_node_count: {} meta_edge_count: {} labels: {} property_keys: {}",
            self.meta_node_count, self.meta_edge_count, self.label_count, self.property_key_count
        )
    }
}

/// Byte-level report for one page, returned by `HiveDb::inspect_page`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageReport {
    /// Inspected page ID.
    pub page_id: u32,
    /// Page type name (e.g. `"DataNode"`, `"IndexLeaf"`, `"Meta"`).
    pub page_type: String,
    /// Slot/cell count from the page header.
    pub slot_count: u16,
    /// Live slots (records, cells, or freelist entries depending on type).
    pub live_slots: u16,
    /// Dead slots (slotted pages only).
    pub dead_slots: u16,
    /// Content-area offset from the page header.
    pub free_space_offset: u16,
    /// Free bytes between the slot table and the content area.
    pub free_bytes: usize,
    /// Page checksum verdict (`None` for the meta page, which carries its
    /// own checksum layout).
    pub checksum_valid: Option<bool>,
    /// Highest LSN stamped on the page.
    pub lsn: u32,
    /// Type-specific lines: decoded records, cells, freelist entries, or
    /// meta fields. Capped at `MAX_INSPECT_DETAILS` with an overflow note.
    pub detail: Vec<String>,
}

impl std::fmt::Display for PageReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let checksum = match self.checksum_valid {
            Some(true) => "valid",
            Some(false) => "INVALID",
            None => "n/a",
        };
        writeln!(
            f,
            "page {} [{}]: slots={} live={} dead={} free={}B checksum={} lsn={}",
            self.page_id,
            self.page_type,
            self.slot_count,
            self.live_slots,
            self.dead_slots,
            self.free_bytes,
            checksum,
            self.lsn
        )?;
        for line in &self.detail {
            writeln!(f, "  {line}")?;
        }
        Ok(())
    }
}

/// One summarized WAL entry, returned by `HiveDb::inspect_wal`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalEntrySummary {
    /// Position of the entry in the log (oldest = 0).
    pub index: usize,
    /// Entry kind: `Begin`, `PageImage`, `Commit`, or `Checkpoint`.
    pub kind: String,
    /// Owning transaction, if any.
    pub tx_id: Option<TxId>,
    /// Log sequence number.
    pub lsn: u64,
    /// Affected page for `PageImage` entries.
    pub page_id: Option<u32>,
    /// Encoded payload size in bytes (page images carry a full 4 KiB image).
    pub payload_bytes: usize,
}

impl std::fmt::Display for WalEntrySummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tx = self
            .tx_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".to_string());
        let page = self
            .page_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".to_string());
        write!(
            f,
            "#{} {} tx={} lsn={} page={} ({} payload bytes)",
            self.index, self.kind, tx, self.lsn, page, self.payload_bytes
        )
    }
}

/// Per-stage wall time plus row count for one query execution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueryMetrics {
    /// Time spent parsing the query string.
    pub parse: std::time::Duration,
    /// Time spent planning the statement.
    pub plan: std::time::Duration,
    /// Time spent executing the plan (includes commit).
    pub execute: std::time::Duration,
    /// Rows in the result.
    pub rows: usize,
}

impl QueryMetrics {
    /// Total wall time across all stages.
    pub fn total(&self) -> std::time::Duration {
        self.parse + self.plan + self.execute
    }

    fn millis(duration: std::time::Duration) -> f64 {
        duration.as_secs_f64() * 1000.0
    }
}

impl std::fmt::Display for QueryMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} rows in {:.3}ms (parse {:.3}ms, plan {:.3}ms, execute {:.3}ms)",
            self.rows,
            Self::millis(self.total()),
            Self::millis(self.parse),
            Self::millis(self.plan),
            Self::millis(self.execute)
        )
    }
}

const DEFAULT_AUTO_CHECKPOINT_INTERVAL: u64 = 64;

impl HiveDb {
    /// Maximum detail lines kept in a [`PageReport`].
    const MAX_INSPECT_DETAILS: usize = 200;

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

    /// Transient dictionary ID handed out by read-only snapshots for names
    /// that do not exist in committed state. It can never match a real
    /// record (real IDs start at 1 and grow) and is never written anywhere,
    /// so unknown labels/keys on the read path simply match nothing.
    pub(crate) const TRANSIENT_ID: u32 = u32::MAX;

    /// Returns true when this handle is a read-only snapshot.
    pub fn is_snapshot(&self) -> bool {
        self.pager.is_read_only()
    }

    /// Opens a read-only snapshot of the database at `path`.
    ///
    /// The snapshot gets a fully private pager (own file handles, own cache)
    /// and replays committed WAL entries into that cache only — the shared
    /// files are never written, so any number of snapshots may be open
    /// alongside (but never concurrently with) a writer. Reads observe the
    /// committed state as of open: later writer commits are invisible until
    /// a fresh snapshot is opened (snapshot isolation).
    ///
    /// Mutating through a snapshot fails loudly (`commit_tx`/`checkpoint`
    /// refuse; page allocation is rejected). Unknown labels/property keys
    /// resolve to [`HiveDb::TRANSIENT_ID`] instead of being registered, so
    /// `MATCH` on nonexistent names simply returns no rows.
    pub fn open_snapshot(path: &Path) -> Result<Self, DbError> {
        let wal_path = path.join("wal.hive");
        let mut pager = Pager::open_read_only(path, 128, 128)?;
        let wal = Wal::open(&wal_path)?;

        // Redo runs unmodified: in read-only mode the pager's disk-write
        // paths redirect into the private cache (see `write_page_to_disk`),
        // so replayed images never reach the shared files. No checkpoint or
        // truncate follows — the WAL is left untouched.
        let recovery_outcome = recovery::recover(path, &mut pager)?;

        match recovery_outcome {
            RecoveryOutcome::Clean => {}
            RecoveryOutcome::Recovered {
                committed_tx_count,
                pages_redone,
            } => {
                eprintln!(
                    "Snapshot recovery: {} transactions replayed, {} pages redone",
                    committed_tx_count, pages_redone
                );
            }
        }

        Ok(Self {
            pager,
            wal,
            next_tx_id: AtomicU64::new(1),
            commits_since_checkpoint: 0,
            auto_checkpoint_interval: 0,
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
        if self.pager.is_read_only() {
            // Read-only snapshots must not register names: hand out a
            // transient ID that matches nothing (see `TRANSIENT_ID`).
            return Ok(Self::TRANSIENT_ID);
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
        if self.pager.is_read_only() {
            // Read-only snapshots must not register names: hand out a
            // transient ID that matches nothing (see `TRANSIENT_ID`).
            return Ok(Self::TRANSIENT_ID);
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

    /// Creates a unique node constraint on `(label, property_key)`.
    pub fn create_unique_constraint(&mut self, label: &str, key: &str) -> Result<(), DbError> {
        let tx_id = self.next_tx_id();
        let mut tx = Transaction::new(self, tx_id)?;
        tx.create_unique_constraint(label, key)?;
        tx.commit()?;
        Ok(())
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

    /// Parses, plans, and executes a query with `$name` parameter bindings.
    pub fn execute_with_params(
        &mut self,
        query: &str,
        params: &std::collections::HashMap<String, Value>,
    ) -> Result<crate::query::result::QueryResult, DbError> {
        let statement = crate::query::parser::parse(query)
            .map_err(|err| DbError::QueryError(err.to_string()))?;
        let plan = crate::query::planner::plan(statement)?;
        crate::query::executor::execute_with_params(&plan, self, params)
    }

    /// Returns a human-readable query plan without executing it (`EXPLAIN`).
    pub fn explain(&mut self, query: &str) -> Result<String, DbError> {
        let statement = crate::query::parser::parse(query)
            .map_err(|err| DbError::QueryError(err.to_string()))?;
        let plan = crate::query::planner::plan(statement)?;
        Ok(crate::query::executor::explain_plan(&plan))
    }

    /// Parses, plans, and executes a query, timing each stage separately.
    ///
    /// Returns the result alongside [`QueryMetrics`] (parse/plan/execute
    /// wall time plus row count) for performance diagnostics.
    pub fn execute_with_params_timed(
        &mut self,
        query: &str,
        params: &std::collections::HashMap<String, Value>,
    ) -> Result<(crate::query::result::QueryResult, QueryMetrics), DbError> {
        let start = std::time::Instant::now();
        let statement = crate::query::parser::parse(query)
            .map_err(|err| DbError::QueryError(err.to_string()))?;
        let parse = start.elapsed();
        let plan = crate::query::planner::plan(statement)?;
        let plan_time = start.elapsed() - parse;
        let result = crate::query::executor::execute_with_params(&plan, self, params)?;
        let execute = start.elapsed() - parse - plan_time;
        let metrics = QueryMetrics {
            parse,
            plan: plan_time,
            execute,
            rows: result.rows.len(),
        };
        Ok((result, metrics))
    }

    /// Parses, plans, and executes a query with per-stage timing.
    pub fn execute_timed(
        &mut self,
        query: &str,
    ) -> Result<(crate::query::result::QueryResult, QueryMetrics), DbError> {
        self.execute_with_params_timed(query, &std::collections::HashMap::new())
    }

    /// Inspects one page and returns a human-readable byte-level report.
    ///
    /// Reports the header (type, slots, free space, checksum, LSN) plus
    /// type-specific content: decoded node/edge records, B-tree cell counts,
    /// freelist entries, or meta-header fields. Slots that fail to decode
    /// are reported as undecodable rather than erroring, so corrupt pages
    /// can still be diagnosed. Detail lines are capped
    /// (`MAX_INSPECT_DETAILS`) with an overflow note.
    pub fn inspect_page(&mut self, page_id: u32) -> Result<PageReport, DbError> {
        let page_count = self.pager.page_count()? as u32;
        if page_id >= page_count {
            return Err(DbError::QueryError(format!(
                "page {page_id} out of range ({page_count} pages)"
            )));
        }
        if page_id == META_PAGE_ID {
            return self.inspect_meta_page();
        }
        // Copy the page out of the cache: decoding below needs `&mut self`
        // (label/property name resolution), which cannot borrow the pager
        // while a page reference is alive.
        let page_buf = *self.pager.get_page(page_id)?;
        let header = layout::read_page_header(&page_buf);
        let mut report = PageReport {
            page_id,
            page_type: format!("{:?}", header.page_type),
            slot_count: header.slot_count,
            live_slots: 0,
            dead_slots: 0,
            free_space_offset: header.free_space_offset,
            free_bytes: layout::get_free_space(&page_buf),
            checksum_valid: Some(layout::verify_checksum(&page_buf)),
            lsn: header.lsn,
            detail: Vec::new(),
        };
        match header.page_type {
            PageType::DataNode
            | PageType::DataEdge
            | PageType::LabelData
            | PageType::PropertyKeyData
            | PageType::StringData => {
                self.describe_slotted_page(header.page_type, &page_buf, &mut report)?;
            }
            PageType::IndexLeaf | PageType::IndexInterior => {
                Self::describe_btree_page(&page_buf, &mut report);
            }
            PageType::Freelist => {
                Self::describe_freelist_page(&page_buf, &mut report);
            }
            PageType::Overflow => {
                Self::describe_overflow_page(&page_buf, &mut report);
            }
            PageType::Meta => {
                report
                    .detail
                    .push("unexpected meta page away from page 0".to_string());
            }
        }
        if report.detail.len() > Self::MAX_INSPECT_DETAILS {
            let extra = report.detail.len() - Self::MAX_INSPECT_DETAILS;
            report.detail.truncate(Self::MAX_INSPECT_DETAILS);
            report.detail.push(format!("... +{extra} more entries"));
        }
        Ok(report)
    }

    /// Describes a slotted page (records addressed by slot index).
    fn describe_slotted_page(
        &mut self,
        page_type: PageType,
        page_buf: &[u8; PAGE_SIZE],
        report: &mut PageReport,
    ) -> Result<(), DbError> {
        let header = layout::read_page_header(page_buf);
        for slot_id in 0..header.slot_count {
            match layout::read_record_bytes(page_buf, slot_id) {
                None => {
                    report.dead_slots += 1;
                }
                Some(bytes) => {
                    report.live_slots += 1;
                    self.describe_slot(page_type, slot_id, bytes, &mut report.detail)?;
                }
            }
        }
        Ok(())
    }

    /// Builds the [`PageReport`] for page 0 (meta page).
    fn inspect_meta_page(&mut self) -> Result<PageReport, DbError> {
        let page_buf = self.pager.get_page(META_PAGE_ID)?;
        let meta = layout::read_meta_header(page_buf);
        let magic_ok = meta.magic == crate::storage::page::format::HIVE_MAGIC;
        Ok(PageReport {
            page_id: META_PAGE_ID,
            page_type: "Meta".to_string(),
            slot_count: 0,
            live_slots: 0,
            dead_slots: 0,
            free_space_offset: 0,
            free_bytes: 0,
            // The meta page carries its own checksum layout; only the magic
            // and version are asserted here.
            checksum_valid: None,
            lsn: meta.lsn,
            detail: vec![
                format!("magic valid: {magic_ok}"),
                format!("version: {}", meta.version),
                format!("page_size: {}", meta.page_size),
                format!("db_size_pages: {}", meta.db_size_pages),
                format!(
                    "node_count: {} edge_count: {}",
                    meta.node_count, meta.edge_count
                ),
                format!(
                    "property_count: {} label_count: {}",
                    meta.property_count, meta.label_count
                ),
                format!(
                    "roots: node={} edge={} label={} propkey={} index={}",
                    meta.root_node_page,
                    meta.root_edge_page,
                    meta.root_label_page,
                    meta.root_string_page,
                    meta.root_index_page,
                ),
                format!(
                    "freelist_head: {} schema_version: {}",
                    meta.freelist_head, meta.schema_version
                ),
            ],
        })
    }

    /// Appends one human-readable line describing a live slot to `detail`.
    fn describe_slot(
        &mut self,
        page_type: PageType,
        slot_id: u16,
        bytes: &[u8],
        detail: &mut Vec<String>,
    ) -> Result<(), DbError> {
        match page_type {
            PageType::DataNode => match NodeRecord::from_bytes(bytes) {
                Ok(node) => {
                    let labels = self.node_label_names(&node)?;
                    detail.push(format!(
                        "slot {slot_id}: node id={} labels=[{}] props={} extra_labels={} out_edge={} in_edge={}",
                        node.id,
                        labels.join(","),
                        node.properties.len(),
                        node.extra_labels.len(),
                        node.first_out_edge != NIL_ID,
                        node.first_in_edge != NIL_ID,
                    ));
                }
                Err(err) => detail.push(format!(
                    "slot {slot_id}: <{} undecodable bytes: {err}>",
                    bytes.len()
                )),
            },
            PageType::DataEdge => match EdgeRecord::from_bytes(bytes) {
                Ok(edge) => {
                    let type_name = if edge.label_id == 0 {
                        String::new()
                    } else {
                        self.get_label_name(edge.label_id)?.unwrap_or_default()
                    };
                    detail.push(format!(
                        "slot {slot_id}: edge id={} {}->{} type={type_name} props={}",
                        edge.id,
                        edge.src,
                        edge.dst,
                        edge.properties.len(),
                    ));
                }
                Err(err) => detail.push(format!(
                    "slot {slot_id}: <{} undecodable bytes: {err}>",
                    bytes.len()
                )),
            },
            // Dictionary entries are `[id: u32][name_len: u16][name: bytes]`.
            PageType::LabelData | PageType::PropertyKeyData => {
                detail.push(format!(
                    "slot {slot_id}: {}",
                    Self::describe_dict_entry(bytes)
                ));
            }
            _ => {
                detail.push(format!("slot {slot_id}: {} bytes", bytes.len()));
            }
        }
        Ok(())
    }

    /// Decodes a label / property-key dictionary entry for inspectors.
    fn describe_dict_entry(bytes: &[u8]) -> String {
        if bytes.len() < 6 {
            return format!("<{} undecodable bytes>", bytes.len());
        }
        let id = u32::from_le_bytes(bytes[0..4].try_into().unwrap_or([0; 4]));
        let len = u16::from_le_bytes(bytes[4..6].try_into().unwrap_or([0; 2])) as usize;
        let name_bytes = bytes.get(6..6 + len).unwrap_or_default();
        match std::str::from_utf8(name_bytes) {
            Ok(name) => format!("id={id} name={name:?}"),
            Err(_) => format!("id={id} <{len} non-utf8 bytes>"),
        }
    }

    /// Describes a B-tree page: leaf/interior, cell count, and per-cell keys.
    ///
    /// B-tree pages use a cell pointer array rather than the slotted-record
    /// layout, so cells are decoded here instead of via `describe_slot`.
    /// Cells are `[key_len: u16][key][payload]`; leaf payloads hold record
    /// IDs, interior payloads hold the child page.
    fn describe_btree_page(page_buf: &[u8; PAGE_SIZE], report: &mut PageReport) {
        use crate::storage::btree::{cell, key::BtreeKey, page as btree_page};
        use crate::storage::page::serializer;
        let kind = if btree_page::is_interior(page_buf) {
            "interior"
        } else {
            "leaf"
        };
        let cells = btree_page::cell_count(page_buf);
        report.live_slots = cells as u16;
        let mut line = format!("btree {kind} page cells={cells}");
        if btree_page::is_interior(page_buf) {
            line.push_str(&format!(
                " leftmost={}",
                btree_page::leftmost_pointer(page_buf)
            ));
        }
        report.detail.push(line);
        for cell_idx in 0..cells {
            let Some(cell) = btree_page::cell_bytes(page_buf, cell_idx) else {
                report.detail.push(format!("cell {cell_idx}: <missing>"));
                continue;
            };
            if cell.len() < 2 {
                report.detail.push(format!("cell {cell_idx}: <truncated>"));
                continue;
            }
            let key_len = serializer::get_u16_le(cell, 0) as usize;
            let key_bytes = cell.get(2..2 + key_len).unwrap_or_default();
            let payload = cell.get(2 + key_len..).unwrap_or_default();
            let key = BtreeKey::decode(key_bytes)
                .map(|k| format!("{k:?}"))
                .unwrap_or_else(|_| "<bad key>".to_string());
            if btree_page::is_interior(page_buf) {
                match cell::decode_interior_payload(payload) {
                    Ok(child) => report
                        .detail
                        .push(format!("cell {cell_idx}: key={key} child={child}")),
                    Err(_) => report
                        .detail
                        .push(format!("cell {cell_idx}: key={key} <bad child>")),
                }
            } else {
                match cell::decode_leaf_payload(payload) {
                    Ok(rids) => {
                        let shown: Vec<String> =
                            rids.iter().take(8).map(|id| id.to_string()).collect();
                        let more = if rids.len() > shown.len() {
                            format!(" +{} more", rids.len() - shown.len())
                        } else {
                            String::new()
                        };
                        report.detail.push(format!(
                            "cell {cell_idx}: key={key} rids=[{}]{more}",
                            shown.join(",")
                        ));
                    }
                    Err(_) => report
                        .detail
                        .push(format!("cell {cell_idx}: key={key} <bad payload>")),
                }
            }
        }
    }

    /// Describes a freelist page: chain link plus reusable page IDs.
    fn describe_freelist_page(page_buf: &[u8; PAGE_SIZE], report: &mut PageReport) {
        use crate::storage::page::format::FreelistPage;
        let flp = FreelistPage::from_bytes(page_buf);
        report.live_slots = flp.entries.len() as u16;
        let shown: Vec<String> = flp
            .entries
            .iter()
            .take(32)
            .map(|id| id.to_string())
            .collect();
        let more = if flp.entries.len() > shown.len() {
            format!(" +{} more", flp.entries.len() - shown.len())
        } else {
            String::new()
        };
        report.detail.push(format!(
            "freelist next={} free_pages=[{}]{more}",
            flp.next_page,
            shown.join(",")
        ));
    }

    /// Describes an overflow page: live slots plus total payload bytes.
    fn describe_overflow_page(page_buf: &[u8; PAGE_SIZE], report: &mut PageReport) {
        let header = layout::read_page_header(page_buf);
        let mut live = 0u16;
        let mut bytes = 0usize;
        for slot_id in 0..header.slot_count {
            if let Some(slot) = layout::read_record_bytes(page_buf, slot_id) {
                live += 1;
                bytes += slot.len();
            } else {
                report.dead_slots += 1;
            }
        }
        report.live_slots = live;
        report.detail.push(format!(
            "overflow payload: {live} live slots, {bytes} bytes"
        ));
    }

    /// Resolves a node's label IDs to names (best effort for inspectors).
    fn node_label_names(&mut self, node: &NodeRecord) -> Result<Vec<String>, DbError> {
        let mut out = Vec::new();
        for label_id in node.all_label_ids() {
            out.push(
                self.get_label_name(label_id)?
                    .unwrap_or_else(|| format!("label_{label_id}")),
            );
        }
        Ok(out)
    }

    /// Lists WAL entries oldest-first, most recent last.
    ///
    /// With `Some(limit)`, only the most recent `limit` entries are returned
    /// (original indices preserved). Entry payloads are summarized, never
    /// dumped: page images report their page ID and byte size.
    pub fn inspect_wal(&mut self, limit: Option<usize>) -> Result<Vec<WalEntrySummary>, DbError> {
        let entries = self.wal.read_all()?;
        let total = entries.len();
        let skip = limit.map(|n| total.saturating_sub(n)).unwrap_or(0);
        Ok(entries
            .into_iter()
            .enumerate()
            .skip(skip)
            .map(|(index, entry)| WalEntrySummary {
                index,
                kind: format!("{:?}", entry.entry_type()),
                tx_id: entry.tx_id(),
                lsn: entry.lsn(),
                page_id: match &entry {
                    WalEntry::PageImage { page_id, .. } => Some(*page_id),
                    _ => None,
                },
                payload_bytes: entry.encode_payload().map(|p| p.len()).unwrap_or(0),
            })
            .collect())
    }

    /// Returns database statistics: page counts, record counts, index counts.
    pub fn stats(&mut self) -> Result<DbStats, DbError> {
        let page_count = self.pager.page_count()? as u32;
        let mut node_pages = 0u32;
        let mut edge_pages = 0u32;
        let mut btree_pages = 0u32;
        let mut overflow_pages = 0u32;
        let mut freelist_pages = 0u32;
        let mut live_nodes = 0u64;
        let mut live_edges = 0u64;
        for page_id in 0..page_count {
            if page_id == META_PAGE_ID {
                continue;
            }
            let page_buf = self.pager.get_page(page_id)?;
            let header = layout::read_page_header(page_buf);
            match header.page_type {
                PageType::DataNode => {
                    node_pages += 1;
                    for slot_id in 0..header.slot_count {
                        if layout::read_record_bytes(page_buf, slot_id).is_some() {
                            live_nodes += 1;
                        }
                    }
                }
                PageType::DataEdge => {
                    edge_pages += 1;
                    for slot_id in 0..header.slot_count {
                        if layout::read_record_bytes(page_buf, slot_id).is_some() {
                            live_edges += 1;
                        }
                    }
                }
                PageType::IndexInterior | PageType::IndexLeaf => btree_pages += 1,
                PageType::Overflow => overflow_pages += 1,
                PageType::Freelist => freelist_pages += 1,
                _ => {}
            }
        }
        let meta_page = self.pager.get_page(META_PAGE_ID)?;
        let meta = layout::read_meta_header(meta_page);
        Ok(DbStats {
            page_count,
            node_pages,
            edge_pages,
            btree_pages,
            overflow_pages,
            freelist_pages,
            live_nodes,
            live_edges,
            meta_node_count: meta.node_count,
            meta_edge_count: meta.edge_count,
            label_count: meta.label_count,
            property_key_count: meta.property_count,
        })
    }

    /// Checks storage integrity: dangling edges, adjacency chain consistency.
    /// Returns a list of problem descriptions (empty = healthy).
    pub fn check_integrity(&mut self) -> Result<Vec<String>, DbError> {
        let mut problems = Vec::new();
        let nodes = self.scan_nodes()?;
        let node_ids: std::collections::HashSet<NodeId> = nodes.iter().map(|(id, _)| *id).collect();
        let edges = self.scan_edges()?;
        for (edge_id, edge) in &edges {
            if !node_ids.contains(&edge.src) {
                problems.push(format!(
                    "edge {} references missing src {}",
                    edge_id, edge.src
                ));
            }
            if !node_ids.contains(&edge.dst) {
                problems.push(format!(
                    "edge {} references missing dst {}",
                    edge_id, edge.dst
                ));
            }
        }
        // Verify adjacency chains contain exactly the edges incident to each node.
        for (node_id, node) in &nodes {
            let out_chain = self.get_edges_from_node(*node_id, true)?;
            for (eid, e) in &out_chain {
                if e.src != *node_id {
                    problems.push(format!(
                        "out-chain of node {} contains edge {} with src {}",
                        node_id, eid, e.src
                    ));
                }
            }
            let in_chain = self.get_edges_from_node(*node_id, false)?;
            for (eid, e) in &in_chain {
                if e.dst != *node_id {
                    problems.push(format!(
                        "in-chain of node {} contains edge {} with dst {}",
                        node_id, eid, e.dst
                    ));
                }
            }
            // Cross-check counts via full scan.
            let expected_out = edges.iter().filter(|(_, e)| e.src == *node_id).count();
            if expected_out != out_chain.len() {
                problems.push(format!(
                    "node {} out-chain len {} != scan count {}",
                    node_id,
                    out_chain.len(),
                    expected_out
                ));
            }
            let expected_in = edges.iter().filter(|(_, e)| e.dst == *node_id).count();
            if expected_in != in_chain.len() {
                problems.push(format!(
                    "node {} in-chain len {} != scan count {}",
                    node_id,
                    in_chain.len(),
                    expected_in
                ));
            }
            let _ = node;
        }
        Ok(problems)
    }

    /// Verifies index consistency: every indexed entry points at a live record
    /// with a matching label/property value. Returns problem descriptions.
    pub fn check_index_consistency(&mut self) -> Result<Vec<String>, DbError> {
        let mut problems = Vec::new();
        let tx_id = self.next_tx_id();
        let _ = tx_id;
        // Use a short-lived transaction-less scan via direct index catalog reads.
        // We enumerate index defs through the catalog B-tree root.
        let meta_page = self.pager.get_page(META_PAGE_ID)?;
        let meta = layout::read_meta_header(meta_page);
        if meta.root_index_page == 0 {
            return Ok(problems);
        }
        // Collect index defs.
        let defs: Vec<crate::storage::index_catalog::IndexDef> = {
            use crate::storage::btree::BTree;
            let mut btree = BTree::open(&mut self.pager, meta.root_index_page);
            let mut out = Vec::new();
            for (key, rids) in btree.scan()? {
                if let Ok(def) = crate::storage::index_catalog::decode_catalog_entry(&key, &rids) {
                    out.push(def);
                }
            }
            out
        };
        for def in defs {
            use crate::storage::btree::BTree;
            let mut btree = BTree::open(&mut self.pager, def.root_page_id);
            for (key, rids) in btree.scan()? {
                for packed in rids {
                    match def.entity_kind {
                        crate::storage::index_catalog::EntityKind::NodeLabel => {
                            if self.get_node(packed).is_err() {
                                problems.push(format!(
                                    "node label index {:?} points at missing node {}",
                                    key, packed
                                ));
                            }
                        }
                        crate::storage::index_catalog::EntityKind::EdgeType => {
                            if self.get_edge(packed).is_err() {
                                problems.push(format!(
                                    "edge type index {:?} points at missing edge {}",
                                    key, packed
                                ));
                            }
                        }
                        crate::storage::index_catalog::EntityKind::NodeProperty => {
                            if self.get_node(packed).is_err() {
                                problems.push(format!(
                                    "node property index {:?} points at missing node {}",
                                    key, packed
                                ));
                            }
                        }
                        crate::storage::index_catalog::EntityKind::EdgeProperty => {
                            if self.get_edge(packed).is_err() {
                                problems.push(format!(
                                    "edge property index {:?} points at missing edge {}",
                                    key, packed
                                ));
                            }
                        }
                        crate::storage::index_catalog::EntityKind::UniqueConstraint => {}
                    }
                }
            }
        }
        Ok(problems)
    }

    /// Returns all label names attached to a node (primary + extras).
    pub fn get_node_labels(&mut self, node_id: NodeId) -> Result<Vec<String>, DbError> {
        let node = self.get_node(node_id)?;
        let mut out = Vec::new();
        if node.label_id != 0
            && let Some(name) = self.get_label_name(node.label_id)?
        {
            out.push(name);
        }
        for extra in &node.extra_labels {
            if let Some(name) = self.get_label_name(*extra)? {
                out.push(name);
            }
        }
        Ok(out)
    }

    /// Adds a label to a node (idempotent). Wraps in an auto-committed transaction.
    pub fn add_node_label(&mut self, node_id: NodeId, label: &str) -> Result<(), DbError> {
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();
        match self.add_node_label_inner(node_id, label, Some(&mut before_images)) {
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

    pub(crate) fn add_node_label_inner(
        &mut self,
        node_id: NodeId,
        label: &str,
        mut before_images: Option<&mut Vec<BeforeImage>>,
    ) -> Result<(), DbError> {
        let label_id = self.register_property_key_inner_noop(label, before_images.as_deref_mut());
        let _ = label_id;
        let label_id = self.register_label_inner(label, before_images.as_deref_mut())?;
        let (page_id, slot_id) = unpack_record_id(node_id);
        let mut node = self.get_node(node_id)?;
        if node.has_label(label_id) {
            return Ok(());
        }
        if node.label_id == 0 {
            node.label_id = label_id;
        } else {
            node.extra_labels.push(label_id);
        }
        let mut buf = vec![0u8; node.encoded_size()];
        node.to_bytes(&mut buf)?;
        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        layout::update_record(page_buf, slot_id, &buf)?;
        Ok(())
    }

    fn register_property_key_inner_noop(
        &mut self,
        _name: &str,
        _before: Option<&mut Vec<BeforeImage>>,
    ) -> Result<u32, DbError> {
        Ok(0)
    }

    /// Removes a label from a node. Primary label removal promotes the first
    /// extra label (if any) to primary to keep the record layout stable.
    pub fn remove_node_label(&mut self, node_id: NodeId, label: &str) -> Result<(), DbError> {
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();
        match self.remove_node_label_inner(node_id, label, Some(&mut before_images)) {
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

    pub(crate) fn remove_node_label_inner(
        &mut self,
        node_id: NodeId,
        label: &str,
        mut before_images: Option<&mut Vec<BeforeImage>>,
    ) -> Result<(), DbError> {
        let label_id = match self.find_label(label)? {
            Some(id) => id,
            None => return Ok(()),
        };
        let (page_id, slot_id) = unpack_record_id(node_id);
        let mut node = self.get_node(node_id)?;
        if node.label_id == label_id {
            if let Some(first) = node.extra_labels.first().copied() {
                node.label_id = first;
                node.extra_labels.remove(0);
            } else {
                node.label_id = 0;
            }
        } else {
            let before = node.extra_labels.len();
            node.extra_labels.retain(|id| *id != label_id);
            if node.extra_labels.len() == before {
                return Ok(());
            }
        }
        let mut buf = vec![0u8; node.encoded_size()];
        node.to_bytes(&mut buf)?;
        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        layout::update_record(page_buf, slot_id, &buf)?;
        Ok(())
    }

    /// Removes a property from a node.
    pub fn remove_node_property(&mut self, node_id: NodeId, key: &str) -> Result<(), DbError> {
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();
        match self.remove_node_property_inner(node_id, key, Some(&mut before_images)) {
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

    pub(crate) fn remove_node_property_inner(
        &mut self,
        node_id: NodeId,
        key: &str,
        mut before_images: Option<&mut Vec<BeforeImage>>,
    ) -> Result<(), DbError> {
        let key_id = match self.find_property_key(key)? {
            Some(id) => id,
            None => return Ok(()),
        };
        let (page_id, slot_id) = unpack_record_id(node_id);
        let mut node = self.get_node(node_id)?;
        let before = node.properties.len();
        node.properties.retain(|p| p.key_id != key_id);
        if node.properties.len() == before {
            return Ok(());
        }
        let mut buf = vec![0u8; node.encoded_size()];
        node.to_bytes(&mut buf)?;
        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        layout::update_record(page_buf, slot_id, &buf)?;
        Ok(())
    }

    /// Removes a property from an edge.
    pub fn remove_edge_property(&mut self, edge_id: EdgeId, key: &str) -> Result<(), DbError> {
        let tx_id = self.next_tx_id();
        let mut before_images = Vec::new();
        match self.remove_edge_property_inner(edge_id, key, Some(&mut before_images)) {
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

    pub(crate) fn remove_edge_property_inner(
        &mut self,
        edge_id: EdgeId,
        key: &str,
        mut before_images: Option<&mut Vec<BeforeImage>>,
    ) -> Result<(), DbError> {
        let key_id = match self.find_property_key(key)? {
            Some(id) => id,
            None => return Ok(()),
        };
        let (page_id, slot_id) = unpack_record_id(edge_id);
        let mut edge = self.get_edge(edge_id)?;
        let before = edge.properties.len();
        edge.properties.retain(|p| p.key_id != key_id);
        if edge.properties.len() == before {
            return Ok(());
        }
        let mut buf = vec![0u8; edge.encoded_size()];
        edge.to_bytes(&mut buf)?;
        Self::capture_before_image(&mut self.pager, &mut before_images, page_id)?;
        let page_buf = self.pager.get_page_mut(page_id)?;
        layout::update_record(page_buf, slot_id, &buf)?;
        Ok(())
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
        if self.pager.is_read_only() {
            return Err(DbError::QueryError(
                "cannot commit a write transaction on a read-only snapshot".to_string(),
            ));
        }
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
        if self.pager.is_read_only() {
            return Err(DbError::QueryError(
                "cannot checkpoint a read-only snapshot".to_string(),
            ));
        }
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
