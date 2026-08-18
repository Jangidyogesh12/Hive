//! B-tree cursor for exact lookup and forward range scans.

use super::cell::{decode_interior_payload, decode_leaf_payload};
use super::page::{cell_count, is_leaf, leftmost_pointer};
use crate::errors::DbError;
use crate::storage::page::serializer;
use crate::storage::pager::Pager;

/// A frame on the cursor stack representing one level of the tree.
#[derive(Debug, Clone, Copy)]
struct Frame {
    page_id: u32,
    cell_idx: usize,
}

/// A single B-tree entry returned by cursor operations.
pub type BtreeEntry = (Vec<u8>, Vec<u64>);

/// A cursor positioned at a leaf cell.
#[derive(Debug)]
pub struct BtreeCursor {
    stack: Vec<Frame>,
}

impl BtreeCursor {
    /// Creates an unpositioned cursor.
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Positions the cursor on the first leaf cell whose key is >= `key` and
    /// returns the record IDs if it is an exact match.
    pub fn seek_exact(
        &mut self,
        pager: &mut Pager,
        root_page_id: u32,
        key: &[u8],
    ) -> Result<Option<Vec<u64>>, DbError> {
        self.stack.clear();
        let mut page_id = root_page_id;
        loop {
            let buf = pager.get_page(page_id)?;
            if is_leaf(buf) {
                match super::page::find_cell_position(buf, key) {
                    Ok(idx) => {
                        self.stack.push(Frame {
                            page_id,
                            cell_idx: idx,
                        });
                        let cell = super::page::cell_bytes(buf, idx).ok_or(DbError::ReadError)?;
                        let (_, payload) = split_cell(cell)?;
                        return Ok(Some(decode_leaf_payload(payload)?));
                    }
                    Err(idx) => {
                        self.stack.push(Frame {
                            page_id,
                            cell_idx: idx,
                        });
                        return Ok(None);
                    }
                }
            }

            let child = find_child_page(buf, key)?;
            self.stack.push(Frame {
                page_id,
                cell_idx: child.cell_idx,
            });
            page_id = child.page_id;
        }
    }

    /// Positions the cursor at the first leaf cell.
    pub fn seek_first(
        &mut self,
        pager: &mut Pager,
        root_page_id: u32,
    ) -> Result<Option<BtreeEntry>, DbError> {
        self.stack.clear();
        let mut page_id = root_page_id;
        loop {
            let buf = pager.get_page(page_id)?;
            if is_leaf(buf) {
                self.stack.push(Frame {
                    page_id,
                    cell_idx: 0,
                });
                return self.current(pager);
            }
            let child = leftmost_pointer(buf);
            self.stack.push(Frame {
                page_id,
                cell_idx: 0,
            });
            page_id = child;
        }
    }

    /// Advances to the next leaf entry and returns it.
    pub fn next(&mut self, pager: &mut Pager) -> Result<Option<BtreeEntry>, DbError> {
        if self.stack.is_empty() {
            return Ok(None);
        }

        // Advance the leaf frame.
        let leaf = self.stack.last_mut().unwrap();
        leaf.cell_idx += 1;

        loop {
            let leaf_frame = *self.stack.last().unwrap();
            let buf = pager.get_page(leaf_frame.page_id)?;
            if leaf_frame.cell_idx < cell_count(buf) {
                return self.current(pager);
            }

            // Need to move to the next leaf via parent.
            self.stack.pop();
            if self.stack.is_empty() {
                return Ok(None);
            }

            let parent_frame = self.stack.last_mut().unwrap();
            parent_frame.cell_idx += 1;

            let maybe_child = {
                let parent_buf = pager.get_page(parent_frame.page_id)?;
                if parent_frame.cell_idx <= cell_count(parent_buf) {
                    // cell_idx 1 means we moved from the leftmost child into
                    // cell 0's right child, so look at index cell_idx - 1.
                    Some(child_page_at(parent_buf, parent_frame.cell_idx - 1)?)
                } else {
                    None
                }
            };

            if let Some(child) = maybe_child {
                self.descend_to_leftmost_leaf(pager, child)?;
                return self.current(pager);
            }
            // Continue up the stack.
        }
    }

    /// Returns the entry at the current leaf position.
    fn current(&mut self, pager: &mut Pager) -> Result<Option<BtreeEntry>, DbError> {
        let frame = *self.stack.last().ok_or(DbError::ReadError)?;
        let buf = pager.get_page(frame.page_id)?;
        if frame.cell_idx >= cell_count(buf) {
            return Ok(None);
        }
        let cell = super::page::cell_bytes(buf, frame.cell_idx).ok_or(DbError::ReadError)?;
        let (key, payload) = split_cell(cell)?;
        Ok(Some((key.to_vec(), decode_leaf_payload(payload)?)))
    }

    /// Descends from `page_id` to its leftmost leaf, pushing frames.
    fn descend_to_leftmost_leaf(
        &mut self,
        pager: &mut Pager,
        mut page_id: u32,
    ) -> Result<(), DbError> {
        loop {
            let buf = pager.get_page(page_id)?;
            if is_leaf(buf) {
                self.stack.push(Frame {
                    page_id,
                    cell_idx: 0,
                });
                return Ok(());
            }
            let child = leftmost_pointer(buf);
            self.stack.push(Frame {
                page_id,
                cell_idx: 0,
            });
            page_id = child;
        }
    }
}

impl Default for BtreeCursor {
    fn default() -> Self {
        Self::new()
    }
}

struct ChildPage {
    page_id: u32,
    cell_idx: usize,
}

/// Finds the child page to follow for `key` in an interior page.
///
/// Interior cell `i` stores the right child for `[key_i, key_{i+1})`.  The
/// leftmost pointer covers keys `< key_0`.  On an exact key match we follow the
/// matching cell's right child.
fn find_child_page(
    buf: &[u8; crate::storage::page::format::PAGE_SIZE],
    key: &[u8],
) -> Result<ChildPage, DbError> {
    let (page_id, cell_idx) = match super::page::find_cell_position(buf, key) {
        Ok(idx) => (child_page_at(buf, idx)?, idx),
        Err(0) => (leftmost_pointer(buf), 0),
        Err(idx) => (child_page_at(buf, idx - 1)?, idx),
    };
    Ok(ChildPage { page_id, cell_idx })
}

/// Returns the right child pointer for interior cell `idx`.
///
/// Cell `idx` covers the half-open range `[key_idx, key_{idx+1})`.  There is no
/// separate rightmost pointer; the last cell covers keys >= the last key.
fn child_page_at(
    buf: &[u8; crate::storage::page::format::PAGE_SIZE],
    idx: usize,
) -> Result<u32, DbError> {
    let cell = super::page::cell_bytes(buf, idx).ok_or(DbError::ReadError)?;
    let (_, payload) = split_cell(cell)?;
    decode_interior_payload(payload)
}

/// Splits a cell into key bytes and payload bytes.
fn split_cell(cell: &[u8]) -> Result<(&[u8], &[u8]), DbError> {
    if cell.len() < 2 {
        return Err(DbError::ReadError);
    }
    let key_len = serializer::get_u16_le(cell, 0) as usize;
    if cell.len() < 2 + key_len {
        return Err(DbError::ReadError);
    }
    let key = &cell[2..2 + key_len];
    let payload = &cell[2 + key_len..];
    Ok((key, payload))
}
