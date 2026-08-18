//! B-tree insertion with page splitting and root growth.

use super::cell::{
    decode_interior_payload, encode_interior_payload, encode_leaf_payload, leaf_payload_add,
};
use super::key::cmp_encoded;
use super::page::{
    cell_bytes, extract_cells, find_cell_position, free_space, init_interior_page, insert_cell,
    is_leaf, leftmost_pointer, rebuild_page, set_leftmost_pointer,
};
use crate::errors::DbError;
use crate::storage::page::format::{PAGE_SIZE, PageType};
use crate::storage::page::serializer;
use crate::storage::pager::Pager;

/// The result of splitting a page: the new right sibling and the key to insert
/// into the parent.
pub struct SplitResult {
    pub right_page_id: u32,
    pub divider_key: Vec<u8>,
}

/// Inserts `(key, record_id)` into the tree rooted at `root_page_id`.  Returns
/// the (possibly changed) root page id.
pub fn insert(
    pager: &mut Pager,
    root_page_id: u32,
    key: &[u8],
    record_id: u64,
) -> Result<u32, DbError> {
    let mut path: Vec<u32> = Vec::new();
    let mut current = root_page_id;

    // Descend to the leaf, recording the path.
    loop {
        path.push(current);
        let is_leaf_page = {
            let buf = pager.get_page(current)?;
            is_leaf(buf)
        };
        if is_leaf_page {
            break;
        }
        current = {
            let buf = pager.get_page(current)?;
            child_for_key(buf, key)?
        };
    }

    let leaf_id = *path.last().unwrap();
    let split = insert_into_leaf(pager, leaf_id, key, record_id)?;

    let mut propagated = split;
    let mut level = path.len() - 1;

    while let Some(split) = propagated {
        if level == 0 {
            // Root split: allocate a new root.
            let new_root = pager.allocate_page()?;
            let new_root_buf = pager.get_page_mut(new_root)?;
            init_interior_page(new_root_buf);
            // Cell 0 stores the divider key and points to the new right sibling,
            // which holds keys >= divider.  The leftmost pointer is the old root
            // (left), which holds keys < divider.
            insert_cell(
                new_root_buf,
                0,
                &split.divider_key,
                &encode_interior_payload(split.right_page_id),
            )?;
            set_leftmost_pointer(new_root_buf, root_page_id);
            return Ok(new_root);
        }

        level -= 1;
        let parent_id = path[level];
        propagated =
            insert_into_interior(pager, parent_id, &split.divider_key, split.right_page_id)?;
    }

    Ok(root_page_id)
}

fn insert_into_leaf(
    pager: &mut Pager,
    page_id: u32,
    key: &[u8],
    record_id: u64,
) -> Result<Option<SplitResult>, DbError> {
    let is_leaf_page = {
        let buf = pager.get_page(page_id)?;
        is_leaf(buf)
    };
    if !is_leaf_page {
        return Err(DbError::WriteError);
    }

    let position = {
        let buf = pager.get_page(page_id)?;
        find_cell_position(buf, key)
    };
    match position {
        Ok(idx) => {
            // Exact match: append record id to the existing list.
            let (cell_key, new_payload) = {
                let buf = pager.get_page(page_id)?;
                let cell = cell_bytes(buf, idx).ok_or(DbError::ReadError)?;
                let key_len = serializer::get_u16_le(cell, 0) as usize;
                let cell_key = cell[2..2 + key_len].to_vec();
                let old_payload = &cell[2 + key_len..];
                let new_payload = leaf_payload_add(old_payload, record_id)?;
                (cell_key, new_payload)
            };

            let buf = pager.get_page_mut(page_id)?;
            super::page::delete_cell(buf, idx)?;
            insert_cell(buf, idx, &cell_key, &new_payload)?;
            Ok(None)
        }
        Err(idx) => {
            let payload = encode_leaf_payload(&[record_id]);
            let needed = 2 + key.len() + payload.len() + super::page::CELL_POINTER_SIZE;
            let has_space = {
                let buf = pager.get_page(page_id)?;
                free_space(buf) >= needed
            };
            if has_space {
                let buf = pager.get_page_mut(page_id)?;
                insert_cell(buf, idx, key, &payload)?;
                Ok(None)
            } else {
                split_leaf(pager, page_id, idx, key, &payload)
            }
        }
    }
}

fn insert_into_interior(
    pager: &mut Pager,
    page_id: u32,
    key: &[u8],
    right_child: u32,
) -> Result<Option<SplitResult>, DbError> {
    let payload = encode_interior_payload(right_child);
    let needed = 2 + key.len() + payload.len() + super::page::CELL_POINTER_SIZE;
    let (has_space, idx) = {
        let buf = pager.get_page(page_id)?;
        let has_space = free_space(buf) >= needed;
        let idx = find_cell_position(buf, key).unwrap_or_else(|i| i);
        (has_space, idx)
    };
    if has_space {
        let buf = pager.get_page_mut(page_id)?;
        insert_cell(buf, idx, key, &payload)?;
        Ok(None)
    } else {
        split_interior(pager, page_id, key, right_child)
    }
}

fn split_leaf(
    pager: &mut Pager,
    page_id: u32,
    insert_idx: usize,
    key: &[u8],
    payload: &[u8],
) -> Result<Option<SplitResult>, DbError> {
    let mut cells = {
        let buf = pager.get_page(page_id)?;
        extract_cells(buf)
    };

    cells.insert(insert_idx, (key.to_vec(), payload.to_vec()));

    let split_idx = cells.len() / 2;
    let left_cells = cells[..split_idx].to_vec();
    let right_cells = cells[split_idx..].to_vec();
    let divider_key = right_cells[0].0.clone();

    let right_page_id = pager.allocate_page()?;

    {
        let left_buf = pager.get_page_mut(page_id)?;
        rebuild_page(left_buf, PageType::IndexLeaf, &left_cells, None)?;
    }

    {
        let right_buf = pager.get_page_mut(right_page_id)?;
        rebuild_page(right_buf, PageType::IndexLeaf, &right_cells, None)?;
    }

    Ok(Some(SplitResult {
        right_page_id,
        divider_key,
    }))
}

fn split_interior(
    pager: &mut Pager,
    page_id: u32,
    key: &[u8],
    right_child: u32,
) -> Result<Option<SplitResult>, DbError> {
    let (mut cells, old_leftmost) = {
        let buf = pager.get_page(page_id)?;
        (extract_cells(buf), leftmost_pointer(buf))
    };

    let insert_idx = find_cell_position_from_cells(&cells, key);
    cells.insert(
        insert_idx,
        (key.to_vec(), encode_interior_payload(right_child)),
    );

    // For interior splits, the divider key is promoted and not stored in either
    // child.  Choose the middle key as the divider.
    let split_idx = cells.len() / 2;
    let divider_key = cells[split_idx].0.clone();

    let left_cells = cells[..split_idx].to_vec();
    let right_cells = cells[split_idx + 1..].to_vec();

    let right_page_id = pager.allocate_page()?;

    {
        let left_buf = pager.get_page_mut(page_id)?;
        // Left node keeps the original leftmost pointer.
        rebuild_page(
            left_buf,
            PageType::IndexInterior,
            &left_cells,
            Some(old_leftmost),
        )?;
    }

    {
        let right_buf = pager.get_page_mut(right_page_id)?;
        // Right node's leftmost pointer is the promoted divider's right child,
        // which covers [divider_key, next_key).
        let right_leftmost = decode_interior_payload(&cells[split_idx].1)?;
        rebuild_page(
            right_buf,
            PageType::IndexInterior,
            &right_cells,
            Some(right_leftmost),
        )?;
    }

    Ok(Some(SplitResult {
        right_page_id,
        divider_key,
    }))
}

/// Finds the child page to descend to for `key` in an interior page.
///
/// Interior cell `i` stores the right child for the half-open range
/// `[key_i, key_{i+1})`.  The leftmost pointer covers keys `< key_0`.
pub(crate) fn child_for_key(buf: &[u8; PAGE_SIZE], key: &[u8]) -> Result<u32, DbError> {
    match find_cell_position(buf, key) {
        Ok(idx) => {
            let cell = cell_bytes(buf, idx).ok_or(DbError::ReadError)?;
            let key_len = serializer::get_u16_le(cell, 0) as usize;
            decode_interior_payload(&cell[2 + key_len..])
        }
        Err(0) => Ok(leftmost_pointer(buf)),
        Err(idx) => {
            let cell = cell_bytes(buf, idx - 1).ok_or(DbError::ReadError)?;
            let key_len = serializer::get_u16_le(cell, 0) as usize;
            decode_interior_payload(&cell[2 + key_len..])
        }
    }
}

fn find_cell_position_from_cells(cells: &[(Vec<u8>, Vec<u8>)], key: &[u8]) -> usize {
    cells
        .binary_search_by(|(k, _)| cmp_encoded(k, key))
        .unwrap_or_else(|i| i)
}
