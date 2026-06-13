use crate::db::index::IndexStore;
use crate::db::store_path::{
    EDGE_STORE_FILE, FREE_LIST_EDGE, FREE_LIST_NODE, INDEX_STORE_FILE, LABEL_STORE_FILE,
    META_FILE, NODE_STORE_FILE, PROP_STORE_FILE, STRING_STORE_FILE, WAL_FILE,
};
use crate::errors::DbError;
use crate::store::edge::record::EdgeRecord;
use crate::store::edge::store::EdgeStore;
use crate::store::free_list::FreeList;
use crate::store::header::{self, DbHeader, CURRENT_VERSION};
use crate::store::label_store::LabelStore;
use crate::store::node::record::NodeRecord;
use crate::store::node::store::NodeStore;
use crate::store::property::record::PropertyRecord;
use crate::store::property::store::PropertyStore;
use crate::store::string_store::StringStore;
use crate::types::DELETED;
use crate::types::{EdgeId, NIL_ID, NodeId};
use crate::value::{self, LONG_STRING, Value};
use crate::wal::{Wal, WalEntry, WalProperty};
use std::path::PathBuf;
use std::{fs, io::Error, path::Path};

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
pub struct Property {
    pub key_value: String,
    pub key_hash: u64,
    pub value_type: u8,
    pub value_inline: [u8; 15],
}

#[derive(Debug, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub label: String,
    pub first_out_edge: u64,
    pub first_in_edge: u64,
    pub flags: u32,
    pub properties: Vec<Property>,
}

#[derive(Debug, PartialEq)]
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

impl HiveDb {
    /// Ensures the database directory exists, creating it recursively if necessary.
    fn ensure_db_dir(path: &Path) -> Result<(), Error> {
        return fs::create_dir_all(path);
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

        let mut node_store = NodeStore::open(&node_store_path)?;
        let mut edge_store = EdgeStore::open(&edge_store_path)?;
        let mut property_store = PropertyStore::open(&prop_store_path)?;
        let mut string_store = StringStore::open(&string_store_path)?;
        let label_store = LabelStore::open(&label_store_path)?;
        let node_free_list = FreeList::open(&node_free_list_path)?;
        let edge_free_list = FreeList::open(&edge_free_list_path)?;
        let mut wal = Wal::open(&wal_path)?;
        let wal_entries = wal.read_all()?;
        if matches!(wal_entries.last(), None | Some(WalEntry::Checkpoint)) {
            wal.truncate()?;
        }
        let index_store = IndexStore::load_or_rebuild(
            &index_path,
            &mut node_store,
            &mut edge_store,
            &mut property_store,
            &mut string_store,
        )?;

        Ok(Self {
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
            index_store,
            wal,
        })
    }

    fn append_wal(&mut self, entry: &WalEntry) -> Result<(), DbError> {
        self.wal.append(entry)?;
        self.wal.sync()
    }

    fn checkpoint_wal(&mut self) -> Result<(), DbError> {
        self.append_wal(&WalEntry::Checkpoint)
    }

    fn properties_to_wal(properties: &[Property]) -> Result<Vec<WalProperty>, DbError> {
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

    /// Writes the current in-memory header to the meta file.
    fn flush_header(&mut self) -> Result<(), DbError> {
        header::write_header(&self.meta_path, self.header)
    }

    /// Closes the database, writing the final header state to disk.
    pub fn close(self) {
        let _ = header::write_header(&self.meta_path, self.header);
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

        let mut src_node = self.node_store.read(src)?;
        edge_record.next_out_edge = src_node.first_out_edge;
        src_node.first_out_edge = edge_id;
        self.node_store.update(src, src_node)?;

        let mut dst_node = self.node_store.read(dst)?;
        edge_record.next_in_edge = dst_node.first_in_edge;
        dst_node.first_in_edge = edge_id;
        self.node_store.update(dst, dst_node)?;

        self.edge_store.append(edge_record)?;

        self.header.edge_count += 1;
        self.header.property_count += prop_count;
        self.flush_header()?;
        self.index_store.insert_edge(edge_id, &edge_record);
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

        let node = self.node_store.read(node_id)?;
        let key_hash = value::hash_key(key);
        let key_offset = self.string_store.append(key)?;
        let old_value = self.get_node_property(node_id, key)?;

        let (value_type, mut value_inline) = value.to_inline_bytes();

        if value_type == LONG_STRING {
            if let Value::String(ref s) = value {
                let offset = self.string_store.append(s)?;
                value_inline[..8].copy_from_slice(&offset.to_le_bytes());
            }
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
                self.index_store
                    .upsert_node_property(node_id, key_hash, old_value.as_ref(), &value);
                self.persist_indexes()?;
                self.checkpoint_wal()?;
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

        self.property_store.append(record)?;
        self.header.property_count += 1;
        self.flush_header()?;
        self.index_store
            .upsert_node_property(node_id, key_hash, old_value.as_ref(), &value);
        self.persist_indexes()?;
        self.checkpoint_wal()?;
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

        let edge = self.edge_store.read(edge_id)?;
        let key_hash = value::hash_key(key);
        let key_offset = self.string_store.append(key)?;

        let (value_type, mut value_inline) = value.to_inline_bytes();

        if value_type == LONG_STRING {
            if let Value::String(ref s) = value {
                let offset = self.string_store.append(s)?;
                value_inline[..8].copy_from_slice(&offset.to_le_bytes());
            }
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
                self.checkpoint_wal()?;
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

        self.property_store.append(record)?;
        self.header.property_count += 1;
        self.flush_header()?;
        self.checkpoint_wal()?;
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
            self.index_store.remove_edge(id, &record);
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

    pub fn lookup_node_ids_by_label(&self, label: &str) -> Result<Vec<NodeId>, DbError> {
        match self.label_store.get_id(label) {
            Some(label_id) => Ok(self.index_store.lookup_nodes_by_label_id(label_id)),
            None => Ok(Vec::new()),
        }
    }

    pub fn lookup_node_ids_by_property(
        &self,
        key: &str,
        value: &Value,
    ) -> Result<Vec<NodeId>, DbError> {
        Ok(self
            .index_store
            .lookup_nodes_by_property(value::hash_key(key), value))
    }

    pub fn lookup_edge_ids_by_type(&self, edge_type: &str) -> Result<Vec<EdgeId>, DbError> {
        match self.label_store.get_id(edge_type) {
            Some(label_id) => Ok(self.index_store.lookup_edges_by_type_id(label_id)),
            None => Ok(Vec::new()),
        }
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
