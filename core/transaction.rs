use crate::db::hive_db::{HiveDb, Property};
use crate::errors::DbError;
use crate::types::{EdgeId, NodeId};
use crate::value::Value;
use crate::wal::{WalEntry, WalProperty};

pub struct Transaction<'db> {
    db: &'db mut HiveDb,
    entries: Vec<WalEntry>,
    available_node_ids: Vec<NodeId>,
    available_edge_ids: Vec<EdgeId>,
    next_node_id: NodeId,
    next_edge_id: EdgeId,
}

impl<'db> Transaction<'db> {
    pub(crate) fn new(db: &'db mut HiveDb) -> Result<Self, DbError> {
        Ok(Self {
            next_node_id: db.node_count()?,
            next_edge_id: db.edge_count()?,
            available_node_ids: db.node_free_ids_snapshot(),
            available_edge_ids: db.edge_free_ids_snapshot(),
            db,
            entries: Vec::new(),
        })
    }

    pub fn create_node(&mut self, label: &str, props: Vec<Property>) -> Result<NodeId, DbError> {
        let node_id = self.reserve_node_id();
        self.entries.push(WalEntry::CreateNode {
            node_id,
            label: label.to_string(),
            properties: HiveDb::properties_to_wal(&props)?,
        });
        Ok(node_id)
    }

    pub fn create_edge(
        &mut self,
        src: NodeId,
        dst: NodeId,
        label: &str,
        props: Vec<Property>,
    ) -> Result<EdgeId, DbError> {
        let edge_id = self.reserve_edge_id();
        self.entries.push(WalEntry::CreateEdge {
            edge_id,
            src,
            dst,
            label: label.to_string(),
            properties: HiveDb::properties_to_wal(&props)?,
        });
        Ok(edge_id)
    }

    pub fn set_node_property(
        &mut self,
        node_id: NodeId,
        key: &str,
        value: Value,
    ) -> Result<(), DbError> {
        self.entries.push(WalEntry::UpdateNode {
            node_id,
            key: key.to_string(),
            value,
        });
        Ok(())
    }

    pub fn set_edge_property(
        &mut self,
        edge_id: EdgeId,
        key: &str,
        value: Value,
    ) -> Result<(), DbError> {
        self.entries.push(WalEntry::UpdateEdge {
            edge_id,
            key: key.to_string(),
            value,
        });
        Ok(())
    }

    pub fn delete_node(&mut self, node_id: NodeId) -> Result<NodeId, DbError> {
        self.entries.push(WalEntry::DeleteNode { node_id });
        Ok(node_id)
    }

    pub fn delete_edge(&mut self, edge_id: EdgeId) -> Result<EdgeId, DbError> {
        self.entries.push(WalEntry::DeleteEdge { edge_id });
        Ok(edge_id)
    }

    pub fn commit(self) -> Result<(), DbError> {
        self.db.commit_transaction_entries(self.entries)
    }

    pub fn rollback(self) {}

    fn reserve_node_id(&mut self) -> NodeId {
        self.available_node_ids.pop().unwrap_or_else(|| {
            let node_id = self.next_node_id;
            self.next_node_id += 1;
            node_id
        })
    }

    fn reserve_edge_id(&mut self) -> EdgeId {
        self.available_edge_ids.pop().unwrap_or_else(|| {
            let edge_id = self.next_edge_id;
            self.next_edge_id += 1;
            edge_id
        })
    }
}

impl From<(String, Value)> for WalProperty {
    fn from(value: (String, Value)) -> Self {
        Self {
            key: value.0,
            value: value.1,
        }
    }
}
