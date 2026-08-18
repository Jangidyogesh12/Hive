//! Durable B+ tree page storage for Hive indexes.
//!
//! The B-tree is built directly on top of the page pager so that every page
//! mutation is tracked by the existing dirty-page and WAL machinery.  Keys are
//! canonically encoded so that byte order matches logical order.

pub mod cell;
pub mod cursor;
pub mod delete;
pub mod insert;
pub mod key;
pub mod page;

use crate::errors::DbError;
use crate::storage::page::serializer;
use crate::storage::pager::Pager;
pub use key::BtreeKey;

/// A record id stored in a leaf cell.  For graph indexes this is the packed
/// node or edge id.
pub type RecordId = u64;

/// A handle to an on-disk B+ tree.
pub struct BTree<'a> {
    pager: &'a mut Pager,
    root_page_id: u32,
}

impl<'a> BTree<'a> {
    /// Creates a new empty B-tree and returns a handle to it.
    pub fn create(pager: &'a mut Pager) -> Result<Self, DbError> {
        let root = pager.allocate_page()?;
        let buf = pager.get_page_mut(root)?;
        page::init_leaf_page(buf);
        Ok(Self {
            pager,
            root_page_id: root,
        })
    }

    /// Opens an existing B-tree by its root page id.
    pub fn open(pager: &'a mut Pager, root_page_id: u32) -> Self {
        Self {
            pager,
            root_page_id,
        }
    }

    /// Inserts a record id under `key`.  Duplicate record ids for the same key
    /// are deduplicated.
    pub fn insert(&mut self, key: &BtreeKey, record_id: RecordId) -> Result<(), DbError> {
        let key_bytes = key.to_bytes();
        self.root_page_id = insert::insert(self.pager, self.root_page_id, &key_bytes, record_id)?;
        Ok(())
    }

    /// Deletes a record id from `key`.  Returns `true` if the pair existed.
    pub fn delete(&mut self, key: &BtreeKey, record_id: RecordId) -> Result<bool, DbError> {
        let key_bytes = key.to_bytes();
        delete::delete(self.pager, self.root_page_id, &key_bytes, record_id)
    }

    /// Looks up all record ids associated with `key`.
    pub fn lookup(&mut self, key: &BtreeKey) -> Result<Option<Vec<RecordId>>, DbError> {
        let key_bytes = key.to_bytes();
        let mut cursor = cursor::BtreeCursor::new();
        cursor.seek_exact(self.pager, self.root_page_id, &key_bytes)
    }

    /// Returns every entry in the tree in key order.
    pub fn scan(&mut self) -> Result<Vec<(BtreeKey, Vec<RecordId>)>, DbError> {
        let mut cursor = cursor::BtreeCursor::new();
        let mut out = Vec::new();
        if let Some((key_bytes, ids)) = cursor.seek_first(self.pager, self.root_page_id)? {
            out.push((BtreeKey::decode(&key_bytes)?, ids));
            while let Some((key_bytes, ids)) = cursor.next(self.pager)? {
                out.push((BtreeKey::decode(&key_bytes)?, ids));
            }
        }
        Ok(out)
    }

    /// Returns the root page id, which may change after a root split.
    pub fn root_page_id(&self) -> u32 {
        self.root_page_id
    }
}

/// Collects all page ids reachable from the tree root.
pub fn collect_pages(pager: &mut Pager, root_page_id: u32) -> Result<Vec<u32>, DbError> {
    let mut out = Vec::new();
    collect_pages_recursive(pager, root_page_id, &mut out)?;
    Ok(out)
}

fn collect_pages_recursive(
    pager: &mut Pager,
    page_id: u32,
    out: &mut Vec<u32>,
) -> Result<(), DbError> {
    out.push(page_id);
    let children = {
        let buf = pager.get_page(page_id)?;
        let mut child_ids = Vec::new();
        if page::is_interior(buf) {
            let leftmost = page::leftmost_pointer(buf);
            if leftmost != 0 {
                child_ids.push(leftmost);
            }
            for i in 0..page::cell_count(buf) {
                let cell = page::cell_bytes(buf, i).ok_or(DbError::ReadError)?;
                let key_len = serializer::get_u16_le(cell, 0) as usize;
                let child = cell::decode_interior_payload(&cell[2 + key_len..])?;
                child_ids.push(child);
            }
        }
        child_ids
    };
    for child in children {
        collect_pages_recursive(pager, child, out)?;
    }
    Ok(())
}
