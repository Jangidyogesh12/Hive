use crate::db::store_path::{
    EDGE_STORE_FILE, LABEL_STORE_FILE, NODE_STORE_FILE, PROP_STORE_FILE, STRING_STORE_FILE,
};
use crate::errors::DbError;
use crate::store::edge_store::EdgeStore;
use crate::store::label_store::LabelStore;
use crate::store::node_store::NodeStore;
use crate::store::property_store::PropertyStore;
use crate::store::string_store::StringStore;
use std::{fs, io::Error, path::Path};

pub struct HiveDb {
    node_store: NodeStore,
    edge_store: EdgeStore,
    property_store: PropertyStore,
    string_store: StringStore,
    label_store: LabelStore,
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
}
