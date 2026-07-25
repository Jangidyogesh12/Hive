use crate::db::hive_db::HiveDb;
use crate::errors::DbError;
use crate::types::{EdgeId, NodeId};
use crate::value::Value;
use crate::wal::wal_entry::TxId;

/// A serializable transaction that tracks page before-images for rollback.
///
/// Created via `HiveDb::begin()`.  All mutations through this transaction
/// record their before-images so the entire transaction can be rolled back
/// on error.  Call `commit` to durably write changes or `rollback` to revert.
pub struct Transaction<'a> {
    db: &'a mut HiveDb,
    tx_id: TxId,
    before_images: Vec<crate::db::hive_db::BeforeImage>,
}

impl<'a> Transaction<'a> {
    /// Creates a new transaction bound to the given database instance.
    pub(crate) fn new(db: &'a mut HiveDb, tx_id: TxId) -> Result<Self, DbError> {
        Ok(Self {
            db,
            tx_id,
            before_images: Vec::new(),
        })
    }

    /// Returns the transaction ID.
    pub fn tx_id(&self) -> TxId {
        self.tx_id
    }

    /// Creates a node as part of this transaction.
    pub fn create_node(&mut self) -> Result<NodeId, DbError> {
        self.create_node_with_label(0)
    }

    /// Creates a labeled node as part of this transaction.
    pub fn create_node_with_label(&mut self, label_id: u32) -> Result<NodeId, DbError> {
        self.db
            .create_node_with_label_inner(label_id, Some(&mut self.before_images))
    }

    /// Creates an edge as part of this transaction.
    pub fn create_edge(&mut self, src_id: NodeId, dst_id: NodeId) -> Result<EdgeId, DbError> {
        self.create_edge_with_label(src_id, dst_id, 0)
    }

    /// Creates a labeled edge as part of this transaction.
    pub fn create_edge_with_label(
        &mut self,
        src_id: NodeId,
        dst_id: NodeId,
        label_id: u32,
    ) -> Result<EdgeId, DbError> {
        self.db.create_edge_with_label_inner(
            src_id,
            dst_id,
            label_id,
            Some(&mut self.before_images),
        )
    }

    /// Sets a node property as part of this transaction.
    pub fn set_node_property(
        &mut self,
        node_id: NodeId,
        key: &str,
        value: &Value,
    ) -> Result<(), DbError> {
        self.db
            .set_node_property_inner(node_id, key, value, Some(&mut self.before_images))
    }

    /// Sets an edge property as part of this transaction.
    pub fn set_edge_property(
        &mut self,
        edge_id: EdgeId,
        key: &str,
        value: &Value,
    ) -> Result<(), DbError> {
        self.db
            .set_edge_property_inner(edge_id, key, value, Some(&mut self.before_images))
    }

    /// Deletes an edge as part of this transaction.
    pub fn delete_edge(&mut self, edge_id: EdgeId) -> Result<(), DbError> {
        self.db
            .delete_edge_inner(edge_id, Some(&mut self.before_images))
    }

    /// Deletes a node as part of this transaction.  Fails if the node has incident edges.
    pub fn delete_node(&mut self, node_id: NodeId) -> Result<(), DbError> {
        self.db
            .delete_node_inner(node_id, Some(&mut self.before_images))
    }

    /// Scans all live node records in the database.
    pub fn scan_nodes(
        &mut self,
    ) -> Result<Vec<(NodeId, crate::storage::page::record::NodeRecord)>, DbError> {
        self.db.scan_nodes()
    }

    /// Scans all live edge records in the database.
    pub fn scan_edges(
        &mut self,
    ) -> Result<Vec<(EdgeId, crate::storage::page::record::EdgeRecord)>, DbError> {
        self.db.scan_edges()
    }

    /// Registers a label name and returns its numeric ID.  Deduplicates automatically.
    pub fn register_label(&mut self, name: &str) -> Result<u32, DbError> {
        self.db
            .register_label_inner(name, Some(&mut self.before_images))
    }

    /// Returns the label name for a given label ID, or `None` if not found.
    pub fn get_label_name(&mut self, label_id: u32) -> Result<Option<String>, DbError> {
        self.db.get_label_name(label_id)
    }

    /// Registers a property-key name and returns its numeric ID.  Deduplicates automatically.
    pub fn register_property_key(&mut self, name: &str) -> Result<u32, DbError> {
        self.db
            .register_property_key_inner(name, Some(&mut self.before_images))
    }

    /// Returns the property-key name for a given `key_id`, or `None` if not found.
    pub fn get_property_key_name(&mut self, key_id: u32) -> Result<Option<String>, DbError> {
        self.db.get_property_key_name(key_id)
    }

    /// Looks up the `key_id` for a given property name, or returns `None` if not found.
    pub fn find_property_key(&mut self, name: &str) -> Result<Option<u32>, DbError> {
        self.db.find_property_key(name)
    }

    /// Reads a node inside this transaction.
    pub fn get_node(
        &mut self,
        node_id: NodeId,
    ) -> Result<crate::storage::page::record::NodeRecord, DbError> {
        self.db.get_node(node_id)
    }

    /// Reads an edge inside this transaction.
    pub fn get_edge(
        &mut self,
        edge_id: EdgeId,
    ) -> Result<crate::storage::page::record::EdgeRecord, DbError> {
        self.db.get_edge(edge_id)
    }

    /// Reads a node property inside this transaction.
    pub fn get_node_property(&mut self, node_id: NodeId, key: &str) -> Result<Value, DbError> {
        self.db.get_node_property(node_id, key)
    }

    /// Reads an edge property inside this transaction.
    pub fn get_edge_property(&mut self, edge_id: EdgeId, key: &str) -> Result<Value, DbError> {
        self.db.get_edge_property(edge_id, key)
    }

    /// Lists all properties on a node as (key_name, value) pairs.
    pub fn list_node_properties(
        &mut self,
        node_id: NodeId,
    ) -> Result<Vec<(String, Value)>, DbError> {
        self.db.list_node_properties(node_id)
    }

    /// Lists all properties on an edge as (key_name, value) pairs.
    pub fn list_edge_properties(
        &mut self,
        edge_id: EdgeId,
    ) -> Result<Vec<(String, Value)>, DbError> {
        self.db.list_edge_properties(edge_id)
    }

    /// Commits the transaction by writing dirty page images to the WAL,
    /// syncing, and stamping page LSNs.
    pub fn commit(self) -> Result<(), DbError> {
        self.db.commit_tx(self.tx_id)
    }

    /// Rolls back all page changes made through this transaction.
    pub fn rollback(self) -> Result<(), DbError> {
        self.db.rollback_pages(&self.before_images)
    }
}
