//! B-tree cell payload encoding.
//!
//! Cells are stored on pages as `[key_len: u16][key bytes][payload]` where the
//! payload format depends on whether the page is a leaf or interior page.

use crate::errors::DbError;
use crate::storage::page::serializer;

/// Encodes a leaf cell payload: `[rid_count: u16][rid: u64]*`.
pub fn encode_leaf_payload(record_ids: &[u64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + record_ids.len() * 8);
    out.extend_from_slice(&(record_ids.len() as u16).to_le_bytes());
    for &rid in record_ids {
        out.extend_from_slice(&rid.to_le_bytes());
    }
    out
}

/// Decodes a leaf cell payload into its record IDs.
pub fn decode_leaf_payload(payload: &[u8]) -> Result<Vec<u64>, DbError> {
    if payload.len() < 2 {
        return Err(DbError::ReadError);
    }
    let count = serializer::get_u16_le(payload, 0) as usize;
    let expected = 2 + count * 8;
    if payload.len() != expected {
        return Err(DbError::ReadError);
    }
    let mut ids = Vec::with_capacity(count);
    for i in 0..count {
        ids.push(serializer::get_u64_le(payload, 2 + i * 8));
    }
    Ok(ids)
}

/// Encodes an interior cell payload: `[left_child_page: u32]`.
pub fn encode_interior_payload(left_child: u32) -> Vec<u8> {
    left_child.to_le_bytes().to_vec()
}

/// Decodes an interior cell payload.
pub fn decode_interior_payload(payload: &[u8]) -> Result<u32, DbError> {
    if payload.len() != 4 {
        return Err(DbError::ReadError);
    }
    Ok(serializer::get_u32_le(payload, 0))
}

/// Adds a record ID to an existing leaf cell, returning the new cell payload.
pub fn leaf_payload_add(payload: &[u8], record_id: u64) -> Result<Vec<u8>, DbError> {
    let mut ids = decode_leaf_payload(payload)?;
    if !ids.contains(&record_id) {
        ids.push(record_id);
    }
    Ok(encode_leaf_payload(&ids))
}

/// Removes a record ID from an existing leaf cell, returning the new payload or
/// `None` if the list becomes empty.
pub fn leaf_payload_remove(payload: &[u8], record_id: u64) -> Result<Option<Vec<u8>>, DbError> {
    let mut ids = decode_leaf_payload(payload)?;
    ids.retain(|&id| id != record_id);
    if ids.is_empty() {
        Ok(None)
    } else {
        Ok(Some(encode_leaf_payload(&ids)))
    }
}
