//! B-tree deletion.
//!
//! Deletion removes a record id from a leaf cell.  If the record id list becomes
//! empty the whole cell is removed and its space reclaimed.  This implementation
//! does not rebalance (merge/steal) under-full pages; that is deferred to index
//! maintenance work in Step 13.

use super::cell::leaf_payload_remove;
use super::page::{cell_bytes, cell_count, delete_cell, find_cell_position, insert_cell, is_leaf};
use crate::errors::DbError;
use crate::storage::page::serializer;
use crate::storage::pager::Pager;

/// Removes `record_id` from the list associated with `key`.  Returns `true` if
/// the key/record pair existed.
pub fn delete(
    pager: &mut Pager,
    root_page_id: u32,
    key: &[u8],
    record_id: u64,
) -> Result<bool, DbError> {
    let mut current = root_page_id;
    loop {
        let is_leaf_page = {
            let buf = pager.get_page(current)?;
            is_leaf(buf)
        };
        if is_leaf_page {
            break;
        }
        current = {
            let buf = pager.get_page(current)?;
            super::insert::child_for_key(buf, key)?
        };
    }

    let (idx, cell_key, new_payload_opt) = {
        let buf = pager.get_page(current)?;
        let count = cell_count(buf);
        let idx = match find_cell_position(buf, key) {
            Ok(i) if i < count => i,
            _ => return Ok(false),
        };

        let cell = cell_bytes(buf, idx).ok_or(DbError::ReadError)?;
        let key_len = serializer::get_u16_le(cell, 0) as usize;
        let cell_key = cell[2..2 + key_len].to_vec();
        let payload = &cell[2 + key_len..];
        let new_payload_opt = leaf_payload_remove(payload, record_id)?;
        (idx, cell_key, new_payload_opt)
    };

    let buf = pager.get_page_mut(current)?;
    match new_payload_opt {
        Some(new_payload) => {
            delete_cell(buf, idx)?;
            insert_cell(buf, idx, &cell_key, &new_payload)?;
        }
        None => {
            delete_cell(buf, idx)?;
        }
    }
    Ok(true)
}
