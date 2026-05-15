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
use crate::types::{EdgeId, NIL_ID, NodeId};
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
}
