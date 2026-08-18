use crate::db::hive_db::HiveDb;
use crate::errors::DbError;
use crate::storage::btree::{self, BTree, BtreeKey, RecordId};
use crate::storage::index_catalog::{
    EntityKind, IndexDef, catalog_key, decode_catalog_entry, decode_root_from_record_id,
    encode_root_as_record_id,
};
use crate::storage::page::format::META_PAGE_ID;
use crate::storage::page::layout;
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
        let node_id = self
            .db
            .create_node_with_label_inner(label_id, Some(&mut self.before_images))?;
        self.maintain_indexes_on_node_create(node_id, label_id)?;
        Ok(node_id)
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
        let edge_id = self.db.create_edge_with_label_inner(
            src_id,
            dst_id,
            label_id,
            Some(&mut self.before_images),
        )?;
        self.maintain_indexes_on_edge_create(edge_id, label_id)?;
        Ok(edge_id)
    }

    /// Sets a node property as part of this transaction.
    pub fn set_node_property(
        &mut self,
        node_id: NodeId,
        key: &str,
        value: &Value,
    ) -> Result<(), DbError> {
        let node = self.get_node(node_id)?;
        let old_value = self.get_node_property(node_id, key).ok();
        self.db
            .set_node_property_inner(node_id, key, value, Some(&mut self.before_images))?;
        let key_id = self.find_property_key(key)?.unwrap_or(0);
        self.maintain_indexes_on_node_property_set(
            node_id,
            node.label_id,
            key_id,
            old_value.as_ref(),
            value,
        )?;
        Ok(())
    }

    /// Sets an edge property as part of this transaction.
    pub fn set_edge_property(
        &mut self,
        edge_id: EdgeId,
        key: &str,
        value: &Value,
    ) -> Result<(), DbError> {
        let edge = self.get_edge(edge_id)?;
        let old_value = self.get_edge_property(edge_id, key).ok();
        self.db
            .set_edge_property_inner(edge_id, key, value, Some(&mut self.before_images))?;
        let key_id = self.find_property_key(key)?.unwrap_or(0);
        self.maintain_indexes_on_edge_property_set(
            edge_id,
            edge.label_id,
            key_id,
            old_value.as_ref(),
            value,
        )?;
        Ok(())
    }

    /// Deletes an edge as part of this transaction.
    pub fn delete_edge(&mut self, edge_id: EdgeId) -> Result<(), DbError> {
        let edge = self.get_edge(edge_id)?;
        let props = self.list_edge_properties(edge_id)?;
        let mut indexed_props = Vec::with_capacity(props.len());
        for (name, value) in &props {
            if let Some(key_id) = self.find_property_key(name)?
                && value_to_btree_key(value).is_ok()
            {
                indexed_props.push((key_id, value.clone()));
            }
        }
        self.db
            .delete_edge_inner(edge_id, Some(&mut self.before_images))?;
        self.maintain_indexes_on_edge_delete(edge_id, edge.label_id, &indexed_props)?;
        Ok(())
    }

    /// Deletes a node as part of this transaction.  Fails if the node has incident edges.
    pub fn delete_node(&mut self, node_id: NodeId) -> Result<(), DbError> {
        let node = self.get_node(node_id)?;
        let props = self.list_node_properties(node_id)?;
        let mut indexed_props = Vec::with_capacity(props.len());
        for (name, value) in &props {
            if let Some(key_id) = self.find_property_key(name)?
                && value_to_btree_key(value).is_ok()
            {
                indexed_props.push((key_id, value.clone()));
            }
        }
        self.db
            .delete_node_inner(node_id, Some(&mut self.before_images))?;
        self.maintain_indexes_on_node_delete(node_id, node.label_id, &indexed_props)?;
        Ok(())
    }

    /// Scans all live node records in the database.
    pub fn scan_nodes(
        &mut self,
    ) -> Result<Vec<(NodeId, crate::storage::page::record::NodeRecord)>, DbError> {
        self.db.scan_nodes()
    }

    /// Looks up node ids by label using the node label index, if one exists.
    pub fn lookup_nodes_by_label(&mut self, label: &str) -> Result<Option<Vec<NodeId>>, DbError> {
        let label_id = match self.db.find_label(label)? {
            Some(id) => id,
            None => return Ok(Some(Vec::new())),
        };
        match self.find_index_root(EntityKind::NodeLabel, label_id, 0)? {
            Some(root) => {
                let mut btree = BTree::open(&mut self.db.pager, root);
                Ok(btree
                    .lookup(&BtreeKey::Int(label_id as i64))?
                    .map(|ids| ids.into_iter().map(|id| id as NodeId).collect()))
            }
            None => Ok(None),
        }
    }

    /// Looks up node ids by property value using a global node property index,
    /// if one exists.
    pub fn lookup_nodes_by_property(
        &mut self,
        key: &str,
        value: &Value,
    ) -> Result<Option<Vec<NodeId>>, DbError> {
        let key_id = match self.find_property_key(key)? {
            Some(id) => id,
            None => return Ok(Some(Vec::new())),
        };
        match self.find_index_root(EntityKind::NodeProperty, 0, key_id)? {
            Some(root) => {
                let mut btree = BTree::open(&mut self.db.pager, root);
                Ok(btree
                    .lookup(&value_to_btree_key(value)?)?
                    .map(|ids| ids.into_iter().map(|id| id as NodeId).collect()))
            }
            None => Ok(None),
        }
    }

    /// Looks up node ids by label and property value using a per-label node
    /// property index, if one exists.
    pub fn lookup_nodes_by_label_and_property(
        &mut self,
        label: &str,
        key: &str,
        value: &Value,
    ) -> Result<Option<Vec<NodeId>>, DbError> {
        let label_id = match self.db.find_label(label)? {
            Some(id) => id,
            None => return Ok(Some(Vec::new())),
        };
        let key_id = match self.find_property_key(key)? {
            Some(id) => id,
            None => return Ok(Some(Vec::new())),
        };
        match self.find_index_root(EntityKind::NodeProperty, label_id, key_id)? {
            Some(root) => {
                let mut btree = BTree::open(&mut self.db.pager, root);
                Ok(btree
                    .lookup(&value_to_btree_key(value)?)?
                    .map(|ids| ids.into_iter().map(|id| id as NodeId).collect()))
            }
            None => Ok(None),
        }
    }

    /// Scans all live edge records in the database.
    pub fn scan_edges(
        &mut self,
    ) -> Result<Vec<(EdgeId, crate::storage::page::record::EdgeRecord)>, DbError> {
        self.db.scan_edges()
    }

    /// Walks the adjacency chain from a node and returns its connected edges.
    pub fn get_edges_from_node(
        &mut self,
        node_id: NodeId,
        outgoing: bool,
    ) -> Result<Vec<(EdgeId, crate::storage::page::record::EdgeRecord)>, DbError> {
        self.db.get_edges_from_node(node_id, outgoing)
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

    /// Looks up the label id for a given name, or returns `None` if not found.
    pub fn find_label(&mut self, name: &str) -> Result<Option<u32>, DbError> {
        self.db.find_label(name)
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

    /// Commits a read-only transaction without WAL work.
    pub fn commit_readonly(self) -> Result<(), DbError> {
        self.db.commit_readonly()
    }

    /// Commits the transaction by writing dirty page images to the WAL,
    /// syncing, and stamping page LSNs.
    pub fn commit(self) -> Result<(), DbError> {
        self.db.commit_tx(self.tx_id)
    }

    /// Creates a new empty B-tree as part of this transaction.
    /// Returns the root page id of the new tree.
    pub fn create_btree(&mut self) -> Result<u32, DbError> {
        let root = self.db.pager.allocate_page()?;
        HiveDb::capture_allocated_page(
            &mut self.db.pager,
            &mut Some(&mut self.before_images),
            root,
        )?;
        let buf = self.db.pager.get_page_mut(root)?;
        crate::storage::btree::page::init_leaf_page(buf);
        Ok(root)
    }

    /// Inserts a record id into a B-tree as part of this transaction.
    /// Returns the (possibly changed) root page id.
    pub fn btree_insert(
        &mut self,
        root_page_id: u32,
        key: &BtreeKey,
        rid: RecordId,
    ) -> Result<u32, DbError> {
        let before_pages = btree::collect_pages(&mut self.db.pager, root_page_id)?;
        for page_id in &before_pages {
            HiveDb::capture_before_image(
                &mut self.db.pager,
                &mut Some(&mut self.before_images),
                *page_id,
            )?;
        }

        let new_root = {
            let mut btree = BTree::open(&mut self.db.pager, root_page_id);
            btree.insert(key, rid)?;
            btree.root_page_id()
        };

        let after_pages: std::collections::HashSet<u32> =
            btree::collect_pages(&mut self.db.pager, new_root)?
                .into_iter()
                .collect();
        let before_set: std::collections::HashSet<u32> = before_pages.into_iter().collect();
        for page_id in after_pages.difference(&before_set) {
            HiveDb::capture_allocated_page(
                &mut self.db.pager,
                &mut Some(&mut self.before_images),
                *page_id,
            )?;
        }

        Ok(new_root)
    }

    /// Deletes a record id from a B-tree as part of this transaction.
    /// Returns `true` if the pair existed.
    pub fn btree_delete(
        &mut self,
        root_page_id: u32,
        key: &BtreeKey,
        rid: RecordId,
    ) -> Result<bool, DbError> {
        let before_pages = btree::collect_pages(&mut self.db.pager, root_page_id)?;
        for page_id in &before_pages {
            HiveDb::capture_before_image(
                &mut self.db.pager,
                &mut Some(&mut self.before_images),
                *page_id,
            )?;
        }

        let mut btree = BTree::open(&mut self.db.pager, root_page_id);
        let deleted = btree.delete(key, rid)?;
        Ok(deleted)
    }

    /// Rolls back all page changes made through this transaction.
    pub fn rollback(self) -> Result<(), DbError> {
        self.db.rollback_pages(&self.before_images)
    }

    // ------------------------------------------------------------------
    // Index catalog helpers
    // ------------------------------------------------------------------

    /// Returns the current index catalog root page id, or 0 if none exists.
    fn index_catalog_root(&mut self) -> Result<u32, DbError> {
        let meta_page = self.db.pager.get_page(META_PAGE_ID)?;
        let meta = layout::read_meta_header(meta_page);
        Ok(meta.root_index_page)
    }

    /// Persists a new index catalog root page id.
    fn set_index_catalog_root(&mut self, root: u32) -> Result<(), DbError> {
        self.db
            .update_meta_header(&mut Some(&mut self.before_images), |meta| {
                meta.root_index_page = root;
            })
    }

    /// Ensures the index catalog B-tree exists, creating it if necessary.
    fn ensure_index_catalog(&mut self) -> Result<u32, DbError> {
        let root = self.index_catalog_root()?;
        if root != 0 {
            return Ok(root);
        }
        let new_root = self.create_btree()?;
        self.set_index_catalog_root(new_root)?;
        Ok(new_root)
    }

    /// Creates an index of the given kind and dimensions, or returns the
    /// existing data root page id if one already exists.
    pub fn create_index(
        &mut self,
        entity_kind: EntityKind,
        label_id: u32,
        property_key_id: u32,
    ) -> Result<u32, DbError> {
        let catalog_root = self.ensure_index_catalog()?;
        if let Some(root) =
            self.find_index_root_in_catalog(catalog_root, entity_kind, label_id, property_key_id)?
        {
            return Ok(root);
        }

        let data_root = self.create_btree()?;
        let key = catalog_key(entity_kind, label_id, property_key_id);
        let new_catalog_root =
            self.btree_insert(catalog_root, &key, encode_root_as_record_id(data_root))?;
        self.set_index_catalog_root(new_catalog_root)?;
        Ok(data_root)
    }

    /// Finds the data root page id for an index, if it exists.
    pub fn find_index_root(
        &mut self,
        entity_kind: EntityKind,
        label_id: u32,
        property_key_id: u32,
    ) -> Result<Option<u32>, DbError> {
        let catalog_root = self.index_catalog_root()?;
        if catalog_root == 0 {
            return Ok(None);
        }
        self.find_index_root_in_catalog(catalog_root, entity_kind, label_id, property_key_id)
    }

    fn find_index_root_in_catalog(
        &mut self,
        catalog_root: u32,
        entity_kind: EntityKind,
        label_id: u32,
        property_key_id: u32,
    ) -> Result<Option<u32>, DbError> {
        let key = catalog_key(entity_kind, label_id, property_key_id);
        let mut btree = BTree::open(&mut self.db.pager, catalog_root);
        match btree.lookup(&key)? {
            Some(ids) if !ids.is_empty() => Ok(Some(decode_root_from_record_id(ids[0]))),
            _ => Ok(None),
        }
    }

    /// Scans the catalog and returns all index definitions.
    pub fn list_indexes(&mut self) -> Result<Vec<IndexDef>, DbError> {
        let catalog_root = self.index_catalog_root()?;
        if catalog_root == 0 {
            return Ok(Vec::new());
        }
        let mut btree = BTree::open(&mut self.db.pager, catalog_root);
        let entries = btree.scan()?;
        let mut out = Vec::with_capacity(entries.len());
        for (key, ids) in entries {
            out.push(decode_catalog_entry(&key, &ids)?);
        }
        Ok(out)
    }

    // ------------------------------------------------------------------
    // Index maintenance
    // ------------------------------------------------------------------

    /// Maintains indexes after a node is created.
    pub(crate) fn maintain_indexes_on_node_create(
        &mut self,
        node_id: NodeId,
        label_id: u32,
    ) -> Result<(), DbError> {
        if let Some(root) = self.find_index_root(EntityKind::NodeLabel, label_id, 0)? {
            let mut btree = BTree::open(&mut self.db.pager, root);
            btree.insert(&BtreeKey::Int(label_id as i64), node_id)?;
        }

        if label_id != 0
            && let Some(root) = self.find_index_root(EntityKind::NodeLabel, 0, 0)?
        {
            let mut btree = BTree::open(&mut self.db.pager, root);
            btree.insert(&BtreeKey::Int(label_id as i64), node_id)?;
        }
        Ok(())
    }

    /// Maintains indexes after a node property is set.
    pub(crate) fn maintain_indexes_on_node_property_set(
        &mut self,
        node_id: NodeId,
        label_id: u32,
        key_id: u32,
        old_value: Option<&Value>,
        new_value: &Value,
    ) -> Result<(), DbError> {
        self.maintain_property_indexes(
            EntityKind::NodeProperty,
            node_id,
            label_id,
            key_id,
            old_value,
            new_value,
        )
    }

    /// Maintains indexes after an edge property is set.
    pub(crate) fn maintain_indexes_on_edge_property_set(
        &mut self,
        edge_id: EdgeId,
        label_id: u32,
        key_id: u32,
        old_value: Option<&Value>,
        new_value: &Value,
    ) -> Result<(), DbError> {
        self.maintain_property_indexes(
            EntityKind::EdgeProperty,
            edge_id,
            label_id,
            key_id,
            old_value,
            new_value,
        )
    }

    fn maintain_property_indexes(
        &mut self,
        entity_kind: EntityKind,
        record_id: u64,
        label_id: u32,
        key_id: u32,
        old_value: Option<&Value>,
        new_value: &Value,
    ) -> Result<(), DbError> {
        // Per-label/type property index.
        if let Some(root) = self.find_index_root(entity_kind, label_id, key_id)? {
            let mut btree = BTree::open(&mut self.db.pager, root);
            if let Some(old) = old_value {
                btree.delete(&value_to_btree_key(old)?, record_id)?;
            }
            btree.insert(&value_to_btree_key(new_value)?, record_id)?;
        }

        // Global property index (label_id == 0).
        if label_id != 0
            && let Some(root) = self.find_index_root(entity_kind, 0, key_id)?
        {
            let mut btree = BTree::open(&mut self.db.pager, root);
            if let Some(old) = old_value {
                btree.delete(&value_to_btree_key(old)?, record_id)?;
            }
            btree.insert(&value_to_btree_key(new_value)?, record_id)?;
        }
        Ok(())
    }

    /// Maintains indexes after a node is deleted.  `label_id` is the node's
    /// label and `properties` are its properties before deletion.
    pub(crate) fn maintain_indexes_on_node_delete(
        &mut self,
        node_id: NodeId,
        label_id: u32,
        properties: &[(u32, Value)],
    ) -> Result<(), DbError> {
        self.maintain_indexes_on_entity_delete(
            EntityKind::NodeLabel,
            EntityKind::NodeProperty,
            node_id,
            label_id,
            properties,
        )
    }

    /// Maintains indexes after an edge is deleted.
    pub(crate) fn maintain_indexes_on_edge_delete(
        &mut self,
        edge_id: EdgeId,
        label_id: u32,
        properties: &[(u32, Value)],
    ) -> Result<(), DbError> {
        self.maintain_indexes_on_entity_delete(
            EntityKind::EdgeType,
            EntityKind::EdgeProperty,
            edge_id,
            label_id,
            properties,
        )
    }

    fn maintain_indexes_on_entity_delete(
        &mut self,
        label_kind: EntityKind,
        property_kind: EntityKind,
        record_id: u64,
        label_id: u32,
        properties: &[(u32, Value)],
    ) -> Result<(), DbError> {
        if label_id != 0 {
            if let Some(root) = self.find_index_root(label_kind, label_id, 0)? {
                let mut btree = BTree::open(&mut self.db.pager, root);
                btree.delete(&BtreeKey::Int(label_id as i64), record_id)?;
            }
            if let Some(root) = self.find_index_root(label_kind, 0, 0)? {
                let mut btree = BTree::open(&mut self.db.pager, root);
                btree.delete(&BtreeKey::Int(label_id as i64), record_id)?;
            }
        }

        for (key_id, value) in properties {
            if let Some(root) = self.find_index_root(property_kind, label_id, *key_id)? {
                let mut btree = BTree::open(&mut self.db.pager, root);
                btree.delete(&value_to_btree_key(value)?, record_id)?;
            }
            if label_id != 0
                && let Some(root) = self.find_index_root(property_kind, 0, *key_id)?
            {
                let mut btree = BTree::open(&mut self.db.pager, root);
                btree.delete(&value_to_btree_key(value)?, record_id)?;
            }
        }
        Ok(())
    }

    /// Maintains indexes after an edge is created.
    pub(crate) fn maintain_indexes_on_edge_create(
        &mut self,
        edge_id: EdgeId,
        label_id: u32,
    ) -> Result<(), DbError> {
        if label_id != 0 {
            if let Some(root) = self.find_index_root(EntityKind::EdgeType, label_id, 0)? {
                let mut btree = BTree::open(&mut self.db.pager, root);
                btree.insert(&BtreeKey::Int(label_id as i64), edge_id)?;
            }
            if let Some(root) = self.find_index_root(EntityKind::EdgeType, 0, 0)? {
                let mut btree = BTree::open(&mut self.db.pager, root);
                btree.insert(&BtreeKey::Int(label_id as i64), edge_id)?;
            }
        }
        Ok(())
    }
}

fn value_to_btree_key(value: &Value) -> Result<BtreeKey, DbError> {
    match value {
        Value::Null => Ok(BtreeKey::Null),
        Value::Integer(n) => Ok(BtreeKey::Int(*n)),
        Value::Float(f) => Ok(BtreeKey::Int(f.to_bits() as i64)),
        Value::String(s) => Ok(BtreeKey::Text(s.clone())),
        Value::Boolean(b) => Ok(BtreeKey::Int(*b as i64)),
        Value::Map(_) | Value::List(_) => Err(DbError::QueryError(
            "compound values cannot be indexed".to_string(),
        )),
    }
}
