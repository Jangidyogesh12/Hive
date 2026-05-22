use crate::db::store_path::{
    EDGE_STORE_FILE, LABEL_STORE_FILE, NODE_STORE_FILE, PROP_STORE_FILE, STRING_STORE_FILE,
};
use crate::errors::DbError;
use crate::store::edge::record::EdgeRecord;
use crate::store::edge::store::EdgeStore;
use crate::store::label_store::LabelStore;
use crate::store::node::record::NodeRecord;
use crate::store::node::store::NodeStore;
use crate::store::property::record::PropertyRecord;
use crate::store::property::store::PropertyStore;
use crate::store::string_store::StringStore;
use crate::types::DELETED;
use crate::types::{EdgeId, NIL_ID, NodeId};
use crate::value::{self, LONG_STRING, Value};
use std::{fs, io::Error, path::Path};

pub struct HiveDb {
    node_store: NodeStore,
    edge_store: EdgeStore,
    property_store: PropertyStore,
    string_store: StringStore,
    label_store: LabelStore,
}

#[derive(Debug, PartialEq)]
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
    fn ensure_db_dir(path: &Path) -> Result<(), Error> {
        return fs::create_dir_all(path);
    }

    pub fn open(path: &Path) -> Result<Self, DbError> {
        Self::ensure_db_dir(path)?;

        let node_store_path = path.join(NODE_STORE_FILE);
        let edge_store_path = path.join(EDGE_STORE_FILE);
        let prop_store_path = path.join(PROP_STORE_FILE);
        let string_store_path = path.join(STRING_STORE_FILE);
        let label_store_path = path.join(LABEL_STORE_FILE);

        let node_store = NodeStore::open(&node_store_path)?;
        let edge_store = EdgeStore::open(&edge_store_path)?;
        let property_store = PropertyStore::open(&prop_store_path)?;
        let string_store = StringStore::open(&string_store_path)?;
        let label_store = LabelStore::open(&label_store_path)?;

        Ok(Self {
            node_store,
            edge_store,
            property_store,
            string_store,
            label_store,
        })
    }

    pub fn close(self) {
        // Files are closed automatically when self is dropped. Rust's out of scop behaviour
        // concept of ownersing and borrowing
    }

    pub fn create_node(&mut self, label: &str, props: Vec<Property>) -> Result<NodeId, DbError> {
        let label_id = self.label_store.get_or_create(label)?;
        let node_id = self.node_store.count()?;

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

        Ok(node_id)
    }

    pub fn create_edge(
        &mut self,
        src: u64,
        dst: u64,
        label: &str,
        props: Vec<Property>,
    ) -> Result<EdgeId, DbError> {
        let edge_id = self.edge_store.count()?;
        let label_id = self.label_store.get_or_create(label)?;

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

        Ok(edge_id)
    }

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

    pub fn set_node_property(
        &mut self,
        node_id: NodeId,
        key: &str,
        value: Value,
    ) -> Result<(), DbError> {
        let node = self.node_store.read(node_id)?;
        let key_hash = value::hash_key(key);
        let key_offset = self.string_store.append(key)?;

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
        Ok(())
    }

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

    pub fn set_edge_property(
        &mut self,
        edge_id: EdgeId,
        key: &str,
        value: Value,
    ) -> Result<(), DbError> {
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
        Ok(())
    }

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

    pub fn delete_node(&mut self, id: u64) -> Result<u64, DbError> {
        let mut record = self.node_store.read(id)?;
        record.flags |= DELETED;

        self.node_store.update(id, record)?;

        Ok(id)
    }
    pub fn delete_edge(&mut self, id: u64) -> Result<u64, DbError> {
        let mut record = self.edge_store.read(id)?;

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

        Ok(id)
    }

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
