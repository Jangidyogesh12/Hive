use crate::db::index::IndexStore;
use crate::db::store_path::{
    EDGE_STORE_FILE, FREE_LIST_EDGE, FREE_LIST_NODE, INDEX_STORE_FILE, LABEL_STORE_FILE, META_FILE,
    NODE_STORE_FILE, PROP_STORE_FILE, STRING_STORE_FILE, WAL_FILE,
};
use crate::errors::DbError;
use crate::store::edge::record::EdgeRecord;
use crate::store::edge::store::EdgeStore;
use crate::store::free_list::FreeList;
use crate::store::header::{self, CURRENT_VERSION, DbHeader};
use crate::store::label_store::LabelStore;
use crate::store::node::record::NodeRecord;
use crate::store::node::store::NodeStore;
use crate::store::property::record::PropertyRecord;
use crate::store::property::store::PropertyStore;
use crate::store::string_store::StringStore;
use crate::transaction::Transaction;
use crate::types::DELETED;
use crate::types::{EdgeId, NIL_ID, NodeId};
use crate::value::{self, LONG_STRING, Value};
use crate::wal::{Wal, WalEntry, WalProperty};
use std::path::PathBuf;
use std::{fs, io::Error, path::Path};

/// Open Hive database handle.
///
/// `HiveDb` owns the stores, indexes, free lists, and WAL for one database
/// directory. Mutating methods write logical WAL entries before changing store
/// files and checkpoint after successful durable writes.
pub struct HiveDb {
    header: DbHeader,
    meta_path: PathBuf,
    node_store: NodeStore,
    edge_store: EdgeStore,
    property_store: PropertyStore,
    string_store: StringStore,
    label_store: LabelStore,
    node_free_list: FreeList,
    edge_free_list: FreeList,
    index_path: PathBuf,
    index_store: IndexStore,
    wal: Wal,
}

#[derive(Debug, PartialEq, Clone)]
/// A decoded property attached to a node or edge.
///
/// Values are stored in the same compact representation used by the property
/// store: a type tag plus a 15-byte inline buffer. Use [`Value::from_bytes`] to
/// decode the value when needed.
pub struct Property {
    pub key_value: String,
    pub key_hash: u64,
    pub value_type: u8,
    pub value_inline: [u8; 15],
}

#[derive(Debug, PartialEq)]
/// A decoded node returned by [`HiveDb::get_node`].
pub struct Node {
    pub id: NodeId,
    pub label: String,
    pub first_out_edge: u64,
    pub first_in_edge: u64,
    pub flags: u32,
    pub properties: Vec<Property>,
}

#[derive(Debug, PartialEq)]
/// A decoded edge returned by [`HiveDb::get_edge`].
pub struct Edge {
    pub id: u64,
    pub label: String,
    pub src: u64,
    pub dst: u64,
    pub next_out_edge: u64,
    pub next_in_edge: u64,
    pub flags: u32,
    pub properties: Vec<Property>,
}

/// Snapshot of database statistics returned by [`HiveDb::info`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HiveDbInfo {
    /// Current database format version.
    pub version: u32,
    /// Number of non-deleted nodes tracked by the database header.
    pub live_node_count: u64,
    /// Number of non-deleted edges tracked by the database header.
    pub live_edge_count: u64,
    /// Number of property records tracked by the database header.
    pub property_count: u64,
    /// Physical node records in `nodes.hive`, including deleted records.
    pub node_record_count: u64,
    /// Physical edge records in `edges.hive`, including deleted records.
    pub edge_record_count: u64,
    /// Physical property records in `props.hive`.
    pub property_record_count: u64,
    /// Node IDs currently available for reuse.
    pub free_node_count: usize,
    /// Edge IDs currently available for reuse.
    pub free_edge_count: usize,
}

impl HiveDb {
    /// Ensures the database directory exists, creating it recursively if necessary.
    fn ensure_db_dir(path: &Path) -> Result<(), Error> {
        fs::create_dir_all(path)
    }

    /// Opens (or creates) a Hive database at the given directory path.
    /// Initialises all store files, label store, free lists, and validates
    /// or writes the database header.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        Self::ensure_db_dir(path)?;

        let meta_path = path.join(META_FILE);

        let header = if meta_path.exists() {
            let h = header::read_header(&meta_path)?;
            if h.version != CURRENT_VERSION {
                return Err(DbError::UnsupportedVersion);
            }
            h
        } else {
            let h = DbHeader::new();
            header::write_header(&meta_path, h)?;
            h
        };

        let node_store_path = path.join(NODE_STORE_FILE);
        let edge_store_path = path.join(EDGE_STORE_FILE);
        let prop_store_path = path.join(PROP_STORE_FILE);
        let string_store_path = path.join(STRING_STORE_FILE);
        let label_store_path = path.join(LABEL_STORE_FILE);
        let index_path = path.join(INDEX_STORE_FILE);
        let wal_path = path.join(WAL_FILE);
        let node_free_list_path = path.join(FREE_LIST_NODE);
        let edge_free_list_path = path.join(FREE_LIST_EDGE);

        let node_store = NodeStore::open(&node_store_path)?;
        let edge_store = EdgeStore::open(&edge_store_path)?;
        let property_store = PropertyStore::open(&prop_store_path)?;
        let string_store = StringStore::open(&string_store_path)?;
        let label_store = LabelStore::open(&label_store_path)?;
        let node_free_list = FreeList::open(&node_free_list_path)?;
        let edge_free_list = FreeList::open(&edge_free_list_path)?;
        let mut wal = Wal::open(&wal_path)?;
        let wal_entries = wal.read_all()?;
        let recovery_entries = Self::recovery_entries(&wal_entries);

        let mut db = Self {
            header,
            meta_path,
            node_store,
            edge_store,
            property_store,
            string_store,
            label_store,
            node_free_list,
            edge_free_list,
            index_path,
            index_store: IndexStore::default(),
            wal,
        };

        if recovery_entries.is_empty() {
            db.wal.truncate()?;
            db.index_store = IndexStore::load_or_rebuild(
                &db.index_path,
                &mut db.node_store,
                &mut db.edge_store,
                &mut db.property_store,
                &mut db.string_store,
            )?;
            return Ok(db);
        }

        db.replay_wal_entries(&recovery_entries)?;
        db.finalize_recovery()?;

        Ok(db)
    }

    fn append_wal(&mut self, entry: &WalEntry) -> Result<(), DbError> {
        self.wal.append(entry)?;
        self.wal.sync()
    }

    fn checkpoint_wal(&mut self) -> Result<(), DbError> {
        self.persist_buffered_state()?;
        self.append_wal(&WalEntry::Checkpoint)
    }

    pub(crate) fn append_transaction_wal(&mut self, entries: Vec<WalEntry>) -> Result<(), DbError> {
        self.append_wal(&WalEntry::Transaction { entries })
    }

    fn recovery_entries(entries: &[WalEntry]) -> Vec<WalEntry> {
        let start = entries
            .iter()
            .rposition(|entry| matches!(entry, WalEntry::Checkpoint))
            .map(|idx| idx + 1)
            .unwrap_or(0);
        entries[start..].to_vec()
    }

    pub(crate) fn properties_to_wal(properties: &[Property]) -> Result<Vec<WalProperty>, DbError> {
        properties
            .iter()
            .map(|property| {
                if property.value_type == LONG_STRING {
                    return Err(DbError::WriteError);
                }

                Ok(WalProperty {
                    key: property.key_value.clone(),
                    value: Value::from_bytes(property.value_type, property.value_inline),
                })
            })
            .collect()
    }

    fn persist_indexes(&self) -> Result<(), DbError> {
        self.index_store.save(&self.index_path)
    }

    fn flush_store_buffers(&mut self) -> Result<(), DbError> {
        self.node_store.flush()?;
        self.edge_store.flush()?;
        self.property_store.flush()?;
        self.string_store.flush()?;
        self.label_store.flush()?;
        self.node_free_list.flush()?;
        self.edge_free_list.flush()?;
        Ok(())
    }

    fn persist_buffered_state(&mut self) -> Result<(), DbError> {
        self.flush_store_buffers()?;
        self.node_store.sync()?;
        self.edge_store.sync()?;
        self.property_store.sync()?;
        self.string_store.sync()?;
        self.label_store.sync()?;
        self.node_free_list.persist()?;
        self.edge_free_list.persist()?;
        Ok(())
    }

    fn record_exists(count: u64, id: u64) -> bool {
        id < count
    }

    fn append_property_chain(&mut self, properties: &[WalProperty]) -> Result<u64, DbError> {
        let mut first_property = NIL_ID;
        let mut prev_property = NIL_ID;

        for property in properties {
            let prop_id = self.property_store.count()?;
            let mut record = PropertyRecord::new(prop_id);
            let (value_type, mut value_inline) = property.value.to_inline_bytes();

            record.key_hash = value::hash_key(&property.key);
            record.key_offset = self.string_store.append(&property.key)?;
            record.value_type = value_type;

            if value_type == LONG_STRING
                && let Value::String(ref s) = property.value
            {
                let offset = self.string_store.append(s)?;
                value_inline[..8].copy_from_slice(&offset.to_le_bytes());
            }

            record.value_inline = value_inline;

            self.property_store.append(record)?;

            if first_property == NIL_ID {
                first_property = prop_id;
            }

            if prev_property != NIL_ID {
                let mut prev = self.property_store.read(prev_property)?;
                prev.next_property = prop_id;
                self.property_store.update(prev_property, prev)?;
            }

            prev_property = prop_id;
        }

        Ok(first_property)
    }

    fn replay_wal_entries(&mut self, entries: &[WalEntry]) -> Result<(), DbError> {
        for entry in entries {
            match entry {
                WalEntry::CreateNode {
                    node_id,
                    label,
                    properties,
                } => self.apply_recovered_create_node(*node_id, label, properties)?,
                WalEntry::CreateEdge {
                    edge_id,
                    src,
                    dst,
                    label,
                    properties,
                } => self.apply_recovered_create_edge(*edge_id, *src, *dst, label, properties)?,
                WalEntry::UpdateNode {
                    node_id,
                    key,
                    value,
                } => self.apply_recovered_update_node(*node_id, key, value.clone())?,
                WalEntry::UpdateEdge {
                    edge_id,
                    key,
                    value,
                } => self.apply_recovered_update_edge(*edge_id, key, value.clone())?,
                WalEntry::DeleteNode { node_id } => self.apply_recovered_delete_node(*node_id)?,
                WalEntry::DeleteEdge { edge_id } => self.apply_recovered_delete_edge(*edge_id)?,
                WalEntry::Checkpoint => {}
                WalEntry::Transaction { entries } => self.replay_wal_entries(entries)?,
            }
        }

        Ok(())
    }

    fn apply_recovered_create_node(
        &mut self,
        node_id: NodeId,
        label: &str,
        properties: &[WalProperty],
    ) -> Result<(), DbError> {
        let label_id = self.label_store.get_or_create(label)?;
        let first_property = self.append_property_chain(properties)?;
        let mut node_record = NodeRecord::new(node_id);
        node_record.label_id = label_id;
        node_record.first_property = first_property;

        let node_count = self.node_store.count()?;
        if Self::record_exists(node_count, node_id) {
            self.node_store.update(node_id, node_record)?;
        } else if node_id == node_count {
            self.node_store.append(node_record)?;
        } else {
            return Err(DbError::WriteError);
        }

        Ok(())
    }

    fn apply_recovered_create_edge(
        &mut self,
        edge_id: EdgeId,
        src: NodeId,
        dst: NodeId,
        label: &str,
        properties: &[WalProperty],
    ) -> Result<(), DbError> {
        let label_id = self.label_store.get_or_create(label)?;
        let first_property = self.append_property_chain(properties)?;
        let mut edge_record = EdgeRecord::new(edge_id);
        edge_record.label_id = label_id;
        edge_record.src = src;
        edge_record.dst = dst;
        edge_record.first_property = first_property;

        let edge_count = self.edge_store.count()?;
        if Self::record_exists(edge_count, edge_id) {
            self.edge_store.update(edge_id, edge_record)?;
        } else if edge_id == edge_count {
            self.edge_store.append(edge_record)?;
        } else {
            return Err(DbError::WriteError);
        }

        Ok(())
    }

    fn apply_recovered_update_node(
        &mut self,
        node_id: NodeId,
        key: &str,
        value: Value,
    ) -> Result<(), DbError> {
        self.set_node_property_internal(node_id, key, value)
    }

    fn apply_recovered_update_edge(
        &mut self,
        edge_id: EdgeId,
        key: &str,
        value: Value,
    ) -> Result<(), DbError> {
        self.set_edge_property_internal(edge_id, key, value)
    }

    fn apply_recovered_delete_node(&mut self, node_id: NodeId) -> Result<(), DbError> {
        let mut record = self.node_store.read(node_id)?;
        record.flags |= DELETED;
        self.node_store.update(node_id, record)
    }

    fn apply_recovered_delete_edge(&mut self, edge_id: EdgeId) -> Result<(), DbError> {
        let mut record = self.edge_store.read(edge_id)?;
        record.flags |= DELETED;
        self.edge_store.update(edge_id, record)
    }

    fn rebuild_edge_links(&mut self) -> Result<(), DbError> {
        let node_count = self.node_store.count()?;
        for node_id in 0..node_count {
            let mut node = self.node_store.read(node_id)?;
            node.first_out_edge = NIL_ID;
            node.first_in_edge = NIL_ID;
            self.node_store.update(node_id, node)?;
        }

        let edge_count = self.edge_store.count()?;
        for edge_id in 0..edge_count {
            let mut edge = self.edge_store.read(edge_id)?;
            if (edge.flags & DELETED) != 0 {
                edge.next_out_edge = NIL_ID;
                edge.next_in_edge = NIL_ID;
                self.edge_store.update(edge_id, edge)?;
                continue;
            }

            let mut src_node = self.node_store.read(edge.src)?;
            let mut dst_node = self.node_store.read(edge.dst)?;

            edge.next_out_edge = src_node.first_out_edge;
            edge.next_in_edge = dst_node.first_in_edge;
            src_node.first_out_edge = edge_id;
            dst_node.first_in_edge = edge_id;

            self.edge_store.update(edge_id, edge)?;
            self.node_store.update(edge.src, src_node)?;
            self.node_store.update(edge.dst, dst_node)?;
        }

        Ok(())
    }

    fn rebuild_header_and_free_lists(&mut self) -> Result<(), DbError> {
        let mut live_nodes = 0;
        let mut live_edges = 0;
        let mut free_nodes = Vec::new();
        let mut free_edges = Vec::new();

        let node_count = self.node_store.count()?;
        for node_id in 0..node_count {
            let node = self.node_store.read(node_id)?;
            if (node.flags & DELETED) != 0 {
                free_nodes.push(node_id);
            } else {
                live_nodes += 1;
            }
        }

        let edge_count = self.edge_store.count()?;
        for edge_id in 0..edge_count {
            let edge = self.edge_store.read(edge_id)?;
            if (edge.flags & DELETED) != 0 {
                free_edges.push(edge_id);
            } else {
                live_edges += 1;
            }
        }

        self.header.node_count = live_nodes;
        self.header.edge_count = live_edges;
        self.header.property_count = self.property_store.count()?;
        self.header.free_node_head = free_nodes.last().copied().unwrap_or(0);
        self.header.free_edge_head = free_edges.last().copied().unwrap_or(0);
        self.node_free_list.replace(free_nodes)?;
        self.edge_free_list.replace(free_edges)?;
        self.flush_header()
    }

    fn rebuild_indexes(&mut self) -> Result<(), DbError> {
        self.index_store = IndexStore::rebuild(
            &mut self.node_store,
            &mut self.edge_store,
            &mut self.property_store,
            &mut self.string_store,
        )?;
        self.persist_indexes()
    }

    fn finalize_recovery(&mut self) -> Result<(), DbError> {
        self.rebuild_edge_links()?;
        self.rebuild_header_and_free_lists()?;
        self.rebuild_indexes()?;
        self.checkpoint_wal()?;
        self.wal.truncate()
    }

    pub(crate) fn commit_transaction_entries(
        &mut self,
        entries: Vec<WalEntry>,
    ) -> Result<(), DbError> {
        self.append_transaction_wal(entries.clone())?;
        self.replay_wal_entries(&entries)?;
        self.rebuild_edge_links()?;
        self.rebuild_header_and_free_lists()?;
        self.rebuild_indexes()?;
        self.checkpoint_wal()
    }

    pub(crate) fn node_free_ids_snapshot(&self) -> Vec<u64> {
        self.node_free_list.snapshot()
    }

    pub(crate) fn edge_free_ids_snapshot(&self) -> Vec<u64> {
        self.edge_free_list.snapshot()
    }

    /// Writes the current in-memory header to the meta file.
    fn flush_header(&mut self) -> Result<(), DbError> {
        header::write_header(&self.meta_path, self.header)
    }

    /// Closes the database, writing the final header state to disk.
    /// Flushes buffered state and writes the database header.
    pub fn close(mut self) {
        let _ = self.persist_buffered_state();
        let _ = header::write_header(&self.meta_path, self.header);
    }

    /// Starts a buffered transaction over this database handle.
    ///
    /// The mutable borrow enforces a single writer while the transaction is
    /// active. Call `commit()` to persist the buffered operations or
    /// `rollback()` to discard them.
    pub fn begin(&mut self) -> Result<Transaction<'_>, DbError> {
        Transaction::new(self)
    }

    /// Returns a point-in-time snapshot of database statistics.
    ///
    /// Live counts come from the durable database header. Physical record
    /// counts come from the underlying stores and include logically deleted
    /// records that may be reused through the free lists.
    pub fn info(&mut self) -> Result<HiveDbInfo, DbError> {
        Ok(HiveDbInfo {
            version: self.header.version,
            live_node_count: self.header.node_count,
            live_edge_count: self.header.edge_count,
            property_count: self.header.property_count,
            node_record_count: self.node_store.count()?,
            edge_record_count: self.edge_store.count()?,
            property_record_count: self.property_store.count()?,
            free_node_count: self.node_free_list.len(),
            free_edge_count: self.edge_free_list.len(),
        })
    }

    /// Returns the total number of node records (including deleted) in the store.
    pub fn node_count(&mut self) -> Result<u64, DbError> {
        self.node_store.count()
    }

    /// Returns the total number of edge records (including deleted) in the store.
    pub fn edge_count(&mut self) -> Result<u64, DbError> {
        self.edge_store.count()
    }

    /// Creates a new node with the given label and property list.
    /// Reuses a freed node ID from the free list when available.
    /// Returns the new node's ID.
    pub fn create_node(&mut self, label: &str, props: Vec<Property>) -> Result<NodeId, DbError> {
        let wal_props = Self::properties_to_wal(&props)?;
        let wal_entry = WalEntry::CreateNode {
            node_id: match self.node_free_list.peek() {
                Some(id) => id,
                None => self.node_store.count()?,
            },
            label: label.to_string(),
            properties: wal_props,
        };
        self.append_wal(&wal_entry)?;

        let prop_count = props.len() as u64;
        let label_id = self.label_store.get_or_create(label)?;
        let node_id = match wal_entry {
            WalEntry::CreateNode { node_id, .. } => node_id,
            _ => unreachable!(),
        };

        let mut first_property = NIL_ID;
        let mut prev_property = NIL_ID;

        for prop in props {
            let prop_id = self.property_store.count()?;
            let mut record = PropertyRecord::new(prop_id);

            record.key_hash = prop.key_hash;
            record.key_offset = self.string_store.append(&prop.key_value)?;
            record.value_type = prop.value_type;
            record.value_inline = prop.value_inline;

            self.property_store.append(record)?;

            if first_property == NIL_ID {
                first_property = prop_id;
            }

            if prev_property != NIL_ID {
                let mut prev = self.property_store.read(prev_property)?;
                prev.next_property = prop_id;
                self.property_store.update(prev_property, prev)?;
            }

            prev_property = prop_id;
        }

        let mut node_record = NodeRecord::new(node_id);
        node_record.label_id = label_id;
        node_record.first_property = first_property;
        self.node_store.append(node_record)?;

        self.header.node_count += 1;
        self.header.property_count += prop_count;
        self.flush_header()?;
        self.index_store.insert_node(
            node_id,
            &node_record,
            &mut self.property_store,
            &mut self.string_store,
        )?;
        self.persist_indexes()?;
        self.checkpoint_wal()?;

        Ok(node_id)
    }

    /// Creates a new edge from `src` to `dst` with the given label and property list.
    /// Links the edge into both the source node's out-edge chain and the
    /// destination node's in-edge chain. Reuses a freed edge ID when available.
    /// Returns the new edge's ID.
    pub fn create_edge(
        &mut self,
        src: u64,
        dst: u64,
        label: &str,
        props: Vec<Property>,
    ) -> Result<EdgeId, DbError> {
        let wal_props = Self::properties_to_wal(&props)?;
        let wal_entry = WalEntry::CreateEdge {
            edge_id: match self.edge_free_list.peek() {
                Some(id) => id,
                None => self.edge_store.count()?,
            },
            src,
            dst,
            label: label.to_string(),
            properties: wal_props,
        };
        self.append_wal(&wal_entry)?;

        let prop_count = props.len() as u64;
        let label_id = self.label_store.get_or_create(label)?;
        let edge_id = match wal_entry {
            WalEntry::CreateEdge { edge_id, .. } => edge_id,
            _ => unreachable!(),
        };

        let mut first_property = NIL_ID;
        let mut prev_property = NIL_ID;

        for prop in props {
            let prop_id = self.property_store.count()?;

            let mut record = PropertyRecord::new(prop_id);

            record.key_hash = prop.key_hash;
            record.key_offset = self.string_store.append(&prop.key_value)?;
            record.value_type = prop.value_type;
            record.value_inline = prop.value_inline;

            self.property_store.append(record)?;

            if first_property == NIL_ID {
                first_property = prop_id;
            }

            if prev_property != NIL_ID {
                let mut prev = self.property_store.read(prev_property)?;
                prev.next_property = prop_id;
                self.property_store.update(prev_property, prev)?;
            }

            prev_property = prop_id;
        }

        let mut edge_record = EdgeRecord::new(edge_id);

        edge_record.label_id = label_id;
        edge_record.dst = dst;
        edge_record.src = src;
        edge_record.first_property = first_property;

        let existing_edge_count = self.edge_store.count()?;
        if edge_id == existing_edge_count {
            self.edge_store.append(edge_record)?;
        } else {
            self.edge_store.update(edge_id, edge_record)?;
        }

        let mut src_node = self.node_store.read(src)?;
        edge_record.next_out_edge = src_node.first_out_edge;
        src_node.first_out_edge = edge_id;

        let mut dst_node = self.node_store.read(dst)?;
        edge_record.next_in_edge = dst_node.first_in_edge;
        dst_node.first_in_edge = edge_id;

        self.edge_store.update(edge_id, edge_record)?;
        self.node_store.update(src, src_node)?;
        self.node_store.update(dst, dst_node)?;

        self.header.edge_count += 1;
        self.header.property_count += prop_count;
        self.flush_header()?;
        self.index_store.insert_edge(
            edge_id,
            &edge_record,
            &mut self.property_store,
            &mut self.string_store,
        )?;
        self.persist_indexes()?;
        self.checkpoint_wal()?;

        Ok(edge_id)
    }

    /// Reads a node by ID, resolving its label string and walking its property chain
    /// to collect all properties.
    pub fn get_node(&mut self, node_id: NodeId) -> Result<Node, DbError> {
        let record = self.node_store.read(node_id)?;

        let label = self
            .label_store
            .get_by_id(record.label_id)
            .unwrap_or("<unknown>")
            .to_string();

        let mut properties: Vec<Property> = Vec::new();
        let mut curr = record.first_property;

        while curr != NIL_ID {
            let prop = self.property_store.read(curr)?;
            let key_value = self.string_store.read(prop.key_offset)?;
            properties.push(Property {
                key_value,
                key_hash: prop.key_hash,
                value_type: prop.value_type,
                value_inline: prop.value_inline,
            });
            curr = prop.next_property;
        }

        Ok(Node {
            id: node_id,
            label,
            first_in_edge: record.first_in_edge,
            first_out_edge: record.first_out_edge,
            flags: record.flags,
            properties,
        })
    }

    /// Reads an edge by ID, resolving its label string and walking its property chain
    /// to collect all properties.
    pub fn get_edge(&mut self, edge_id: u64) -> Result<Edge, DbError> {
        let record = self.edge_store.read(edge_id)?;

        let label = self
            .label_store
            .get_by_id(record.label_id)
            .unwrap_or("<unknown>")
            .to_string();

        let mut properties: Vec<Property> = Vec::new();

        let mut curr = record.first_property;

        while curr != NIL_ID {
            let prop = self.property_store.read(curr)?;
            let key_value = self.string_store.read(prop.key_offset)?;
            properties.push(Property {
                key_value,
                key_hash: prop.key_hash,
                value_type: prop.value_type,
                value_inline: prop.value_inline,
            });

            curr = prop.next_property;
        }

        Ok(Edge {
            id: edge_id,
            label,
            dst: record.dst,
            src: record.src,
            next_in_edge: record.next_in_edge,
            next_out_edge: record.next_out_edge,
            flags: record.flags,
            properties,
        })
    }

    /// Sets (or updates) a property on a node by key name.
    /// Long string values are stored externally in the string store.
    pub fn set_node_property(
        &mut self,
        node_id: NodeId,
        key: &str,
        value: Value,
    ) -> Result<(), DbError> {
        self.append_wal(&WalEntry::UpdateNode {
            node_id,
            key: key.to_string(),
            value: value.clone(),
        })?;

        self.set_node_property_internal(node_id, key, value)?;
        self.checkpoint_wal()?;
        Ok(())
    }

    fn set_node_property_internal(
        &mut self,
        node_id: NodeId,
        key: &str,
        value: Value,
    ) -> Result<(), DbError> {
        let node = self.node_store.read(node_id)?;
        let key_hash = value::hash_key(key);
        let key_offset = self.string_store.append(key)?;
        let old_value = self.get_node_property(node_id, key)?;

        let (value_type, mut value_inline) = value.to_inline_bytes();

        if value_type == LONG_STRING
            && let Value::String(ref s) = value
        {
            let offset = self.string_store.append(s)?;
            value_inline[..8].copy_from_slice(&offset.to_le_bytes());
        }

        let mut curr = node.first_property;
        while curr != NIL_ID {
            let prop = self.property_store.read(curr)?;
            if prop.key_hash == key_hash {
                let mut updated = prop;
                updated.value_type = value_type;
                updated.value_inline = value_inline;
                updated.key_offset = key_offset;
                self.property_store.update(curr, updated)?;
                self.index_store.upsert_node_property(
                    node_id,
                    key_hash,
                    old_value.as_ref(),
                    &value,
                );
                self.persist_indexes()?;
                return Ok(());
            }
            curr = prop.next_property;
        }

        let prop_id = self.property_store.count()?;
        let mut record = PropertyRecord::new(prop_id);
        record.key_hash = key_hash;
        record.key_offset = key_offset;
        record.value_type = value_type;
        record.value_inline = value_inline;

        self.property_store.append(record)?;

        if node.first_property == NIL_ID {
            let mut node = node;
            node.first_property = prop_id;
            self.node_store.update(node_id, node)?;
        } else {
            let mut tail = node.first_property;
            loop {
                let prop = self.property_store.read(tail)?;
                if prop.next_property == NIL_ID {
                    break;
                }
                tail = prop.next_property;
            }
            let mut last = self.property_store.read(tail)?;
            last.next_property = prop_id;
            self.property_store.update(tail, last)?;
        }

        self.header.property_count += 1;
        self.flush_header()?;
        self.index_store
            .upsert_node_property(node_id, key_hash, old_value.as_ref(), &value);
        self.persist_indexes()?;
        Ok(())
    }

    /// Retrieves a property value from a node by key name.
    /// Returns `Ok(None)` if the property is not found.
    pub fn get_node_property(
        &mut self,
        node_id: NodeId,
        key: &str,
    ) -> Result<Option<Value>, DbError> {
        let node = self.node_store.read(node_id)?;
        let key_hash = value::hash_key(key);

        let mut curr = node.first_property;
        while curr != NIL_ID {
            let prop = self.property_store.read(curr)?;
            if prop.key_hash == key_hash {
                if prop.value_type == LONG_STRING {
                    let offset = u64::from_le_bytes(prop.value_inline[..8].try_into().unwrap());
                    let s = self.string_store.read(offset)?;
                    return Ok(Some(Value::String(s)));
                }
                return Ok(Some(Value::from_bytes(prop.value_type, prop.value_inline)));
            }
            curr = prop.next_property;
        }

        Ok(None)
    }

    /// Sets (or updates) a property on an edge by key name.
    /// Long string values are stored externally in the string store.
    pub fn set_edge_property(
        &mut self,
        edge_id: EdgeId,
        key: &str,
        value: Value,
    ) -> Result<(), DbError> {
        self.append_wal(&WalEntry::UpdateEdge {
            edge_id,
            key: key.to_string(),
            value: value.clone(),
        })?;

        self.set_edge_property_internal(edge_id, key, value)?;
        self.checkpoint_wal()?;
        Ok(())
    }

    fn set_edge_property_internal(
        &mut self,
        edge_id: EdgeId,
        key: &str,
        value: Value,
    ) -> Result<(), DbError> {
        let edge = self.edge_store.read(edge_id)?;
        let key_hash = value::hash_key(key);
        let key_offset = self.string_store.append(key)?;
        let old_value = self.get_edge_property(edge_id, key)?;

        let (value_type, mut value_inline) = value.to_inline_bytes();

        if value_type == LONG_STRING
            && let Value::String(ref s) = value
        {
            let offset = self.string_store.append(s)?;
            value_inline[..8].copy_from_slice(&offset.to_le_bytes());
        }

        let mut curr = edge.first_property;
        while curr != NIL_ID {
            let prop = self.property_store.read(curr)?;
            if prop.key_hash == key_hash {
                let mut updated = prop;
                updated.value_type = value_type;
                updated.value_inline = value_inline;
                updated.key_offset = key_offset;
                self.property_store.update(curr, updated)?;
                self.index_store.upsert_edge_property(
                    edge_id,
                    key_hash,
                    old_value.as_ref(),
                    &value,
                );
                self.persist_indexes()?;
                return Ok(());
            }
            curr = prop.next_property;
        }

        let prop_id = self.property_store.count()?;
        let mut record = PropertyRecord::new(prop_id);
        record.key_hash = key_hash;
        record.key_offset = key_offset;
        record.value_type = value_type;
        record.value_inline = value_inline;

        self.property_store.append(record)?;

        if edge.first_property == NIL_ID {
            let mut edge = edge;
            edge.first_property = prop_id;
            self.edge_store.update(edge_id, edge)?;
        } else {
            let mut tail = edge.first_property;
            loop {
                let prop = self.property_store.read(tail)?;
                if prop.next_property == NIL_ID {
                    break;
                }
                tail = prop.next_property;
            }
            let mut last = self.property_store.read(tail)?;
            last.next_property = prop_id;
            self.property_store.update(tail, last)?;
        }

        self.header.property_count += 1;
        self.flush_header()?;
        self.index_store
            .upsert_edge_property(edge_id, key_hash, old_value.as_ref(), &value);
        self.persist_indexes()?;
        Ok(())
    }

    /// Retrieves a property value from an edge by key name.
    /// Returns `Ok(None)` if the property is not found.
    pub fn get_edge_property(
        &mut self,
        edge_id: EdgeId,
        key: &str,
    ) -> Result<Option<Value>, DbError> {
        let edge = self.edge_store.read(edge_id)?;
        let key_hash = value::hash_key(key);

        let mut curr = edge.first_property;
        while curr != NIL_ID {
            let prop = self.property_store.read(curr)?;
            if prop.key_hash == key_hash {
                if prop.value_type == LONG_STRING {
                    let offset = u64::from_le_bytes(prop.value_inline[..8].try_into().unwrap());
                    let s = self.string_store.read(offset)?;
                    return Ok(Some(Value::String(s)));
                }
                return Ok(Some(Value::from_bytes(prop.value_type, prop.value_inline)));
            }
            curr = prop.next_property;
        }

        Ok(None)
    }

    /// Marks a node as deleted (sets the DELETED flag) and adds its ID to the free list.
    /// Idempotent: if the node is already deleted, counts are not decremented again.
    pub fn delete_node(&mut self, id: u64) -> Result<u64, DbError> {
        self.append_wal(&WalEntry::DeleteNode { node_id: id })?;

        let mut record = self.node_store.read(id)?;
        let was_deleted = (record.flags & DELETED) != 0;
        if !was_deleted {
            self.index_store.remove_node(
                id,
                &record,
                &mut self.property_store,
                &mut self.string_store,
            )?;
        }
        record.flags |= DELETED;

        self.node_store.update(id, record)?;
        self.node_free_list.push(id)?;
        if !was_deleted {
            self.header.node_count -= 1;
            self.flush_header()?;
            self.persist_indexes()?;
        }
        self.checkpoint_wal()?;
        Ok(id)
    }
    /// Marks an edge as deleted, unlinks it from source and destination node chains,
    /// and adds its ID to the free list. Idempotent on already-deleted edges.
    pub fn delete_edge(&mut self, id: u64) -> Result<u64, DbError> {
        self.append_wal(&WalEntry::DeleteEdge { edge_id: id })?;

        let mut record = self.edge_store.read(id)?;
        let was_deleted = (record.flags & DELETED) != 0;
        if !was_deleted {
            self.index_store.remove_edge(
                id,
                &record,
                &mut self.property_store,
                &mut self.string_store,
            )?;
        }

        // Unlink the source node
        let mut src_node = self.node_store.read(record.src)?;
        if src_node.first_out_edge == id {
            src_node.first_out_edge = record.next_out_edge;
        } else {
            let mut curr = src_node.first_out_edge;
            while curr != NIL_ID {
                let mut curr_edge = self.edge_store.read(curr)?;
                if curr_edge.next_out_edge == id {
                    curr_edge.next_out_edge = record.next_out_edge;
                    self.edge_store.update(curr, curr_edge)?;
                    break;
                }
                curr = curr_edge.next_out_edge;
            }
        }
        self.node_store.update(record.src, src_node)?;

        // Unlink the destination node
        let mut dst_node = self.node_store.read(record.dst)?;
        if dst_node.first_in_edge == id {
            dst_node.first_in_edge = record.next_in_edge;
        } else {
            let mut curr = dst_node.first_in_edge;
            while curr != NIL_ID {
                let mut curr_edge = self.edge_store.read(curr)?;
                if curr_edge.next_in_edge == id {
                    curr_edge.next_in_edge = record.next_in_edge;
                    self.edge_store.update(curr, curr_edge)?;
                    break;
                }
                curr = curr_edge.next_in_edge;
            }
        }
        self.node_store.update(record.dst, dst_node)?;

        // Ste DELETED flag in the edge
        record.flags |= DELETED;
        self.edge_store.update(id, record)?;
        self.edge_free_list.push(id)?;

        if !was_deleted {
            self.header.edge_count -= 1;
            self.flush_header()?;
            self.persist_indexes()?;
        }

        self.checkpoint_wal()?;

        Ok(id)
    }

    /// Returns node IDs currently indexed under the given label.
    pub fn lookup_node_ids_by_label(&self, label: &str) -> Result<Vec<NodeId>, DbError> {
        match self.label_store.get_id(label) {
            Some(label_id) => Ok(self.index_store.lookup_nodes_by_label_id(label_id)),
            None => Ok(Vec::new()),
        }
    }

    /// Returns node IDs currently indexed by an exact property match.
    pub fn lookup_node_ids_by_property(
        &self,
        key: &str,
        value: &Value,
    ) -> Result<Vec<NodeId>, DbError> {
        Ok(self
            .index_store
            .lookup_nodes_by_property(value::hash_key(key), value))
    }

    /// Returns edge IDs currently indexed under the given edge type.
    pub fn lookup_edge_ids_by_type(&self, edge_type: &str) -> Result<Vec<EdgeId>, DbError> {
        match self.label_store.get_id(edge_type) {
            Some(label_id) => Ok(self.index_store.lookup_edges_by_type_id(label_id)),
            None => Ok(Vec::new()),
        }
    }

    /// Returns edge IDs currently indexed by an exact property match.
    pub fn lookup_edge_ids_by_property(
        &self,
        key: &str,
        value: &Value,
    ) -> Result<Vec<EdgeId>, DbError> {
        Ok(self
            .index_store
            .lookup_edges_by_property(value::hash_key(key), value))
    }

    /// Returns all non-deleted destination node IDs reachable via outgoing edges
    /// from the given node.
    pub fn get_out_neighbors(&mut self, id: NodeId) -> Result<Vec<NodeId>, DbError> {
        let mut neighbors: Vec<NodeId> = Vec::new();

        let record = self.node_store.read(id)?;

        let mut curr = record.first_out_edge;

        while curr != NIL_ID {
            let edge_record = self.edge_store.read(curr)?;
            if (edge_record.flags & DELETED) == 0 {
                let dst_node = self.node_store.read(edge_record.dst)?;
                if (dst_node.flags & DELETED) == 0 {
                    neighbors.push(edge_record.dst);
                }
            }
            curr = edge_record.next_out_edge;
        }

        Ok(neighbors)
    }

    /// Returns all non-deleted source node IDs reachable via incoming edges
    /// to the given node.
    pub fn get_in_neighbors(&mut self, id: NodeId) -> Result<Vec<NodeId>, DbError> {
        let mut neighbors: Vec<NodeId> = Vec::new();

        let record = self.node_store.read(id)?;

        let mut curr = record.first_in_edge;

        while curr != NIL_ID {
            let edge_record = self.edge_store.read(curr)?;
            if (edge_record.flags & DELETED) == 0 {
                let src_node = self.node_store.read(edge_record.src)?;
                if (src_node.flags & DELETED) == 0 {
                    neighbors.push(edge_record.src);
                }
            }
            curr = edge_record.next_in_edge;
        }

        Ok(neighbors)
    }
}
