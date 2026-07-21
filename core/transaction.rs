use crate::db::hive_db::HiveDb;
use crate::errors::DbError;
use crate::types::{EdgeId, NodeId};
use crate::value::Value;
use crate::wal::wal_entry::TxId;

pub struct Transaction<'a> {
    db: &'a mut HiveDb,
    tx_id: TxId,
    before_images: Vec<crate::db::hive_db::BeforeImage>,
}

impl<'a> Transaction<'a> {
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

    /// Reads a node inside this transaction.
    pub fn get_node(&mut self, node_id: NodeId) -> Result<crate::storage::page::record::NodeRecord, DbError> {
        self.db.get_node(node_id)
    }

    /// Reads an edge inside this transaction.
    pub fn get_edge(&mut self, edge_id: EdgeId) -> Result<crate::storage::page::record::EdgeRecord, DbError> {
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
