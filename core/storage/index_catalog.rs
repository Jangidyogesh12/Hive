//! Index catalog key encoding.
//!
//! The catalog maps an index definition to the root page id of the B-tree that
//! stores the index data.  It is itself stored as a B-tree whose leaf payloads
//! contain a single "record id" that is really the root page id of the target
//! index encoded as a `u64`.
//!
//! Catalog key layout (composite):
//!     [ entity_kind: i64 ][ label_id: i64 ][ property_key_id: i64 ]
//!
//! `entity_kind` values:
//!     0 = node label index
//!     1 = edge type index
//!     2 = node property index
//!     3 = edge property index
//!
//! For label/type indexes `property_key_id` is 0.  For property indexes without
//! a label/type restriction `label_id` is 0.

use crate::errors::DbError;
use crate::storage::btree::BtreeKey;

/// The kind of entity an index or constraint covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EntityKind {
    NodeLabel = 0,
    EdgeType = 1,
    NodeProperty = 2,
    EdgeProperty = 3,
    UniqueConstraint = 4,
}

impl EntityKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::NodeLabel),
            1 => Some(Self::EdgeType),
            2 => Some(Self::NodeProperty),
            3 => Some(Self::EdgeProperty),
            4 => Some(Self::UniqueConstraint),
            _ => None,
        }
    }
}

/// An index definition as stored in the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDef {
    pub entity_kind: EntityKind,
    pub label_id: u32,
    pub property_key_id: u32,
    pub root_page_id: u32,
}

impl IndexDef {
    /// Returns the composite `BtreeKey` used to locate this index in the catalog.
    pub fn catalog_key(&self) -> BtreeKey {
        catalog_key(self.entity_kind, self.label_id, self.property_key_id)
    }
}

/// Builds a catalog key for the given index dimensions.
pub fn catalog_key(entity_kind: EntityKind, label_id: u32, property_key_id: u32) -> BtreeKey {
    BtreeKey::Composite(vec![
        BtreeKey::Int(entity_kind as i64),
        BtreeKey::Int(label_id as i64),
        BtreeKey::Int(property_key_id as i64),
    ])
}

/// Parses a catalog key back into its components.
pub fn parse_catalog_key(key: &BtreeKey) -> Option<(EntityKind, u32, u32)> {
    match key {
        BtreeKey::Composite(parts) if parts.len() == 3 => {
            let kind = match parts[0] {
                BtreeKey::Int(v) => EntityKind::from_u8(v as u8)?,
                _ => return None,
            };
            let label_id = match parts[1] {
                BtreeKey::Int(v) => v as u32,
                _ => return None,
            };
            let property_key_id = match parts[2] {
                BtreeKey::Int(v) => v as u32,
                _ => return None,
            };
            Some((kind, label_id, property_key_id))
        }
        _ => None,
    }
}

/// Encodes a root page id as the single `u64` payload stored in a catalog leaf.
pub fn encode_root_as_record_id(root_page_id: u32) -> u64 {
    root_page_id as u64
}

/// Decodes the root page id from a catalog leaf payload.
pub fn decode_root_from_record_id(record_id: u64) -> u32 {
    record_id as u32
}

/// Decodes an index definition from a catalog entry returned by `BTree::scan`.
pub fn decode_catalog_entry(key: &BtreeKey, record_ids: &[u64]) -> Result<IndexDef, DbError> {
    let (entity_kind, label_id, property_key_id) =
        parse_catalog_key(key).ok_or(DbError::ReadError)?;
    let root_page_id = record_ids
        .first()
        .copied()
        .map(decode_root_from_record_id)
        .ok_or(DbError::ReadError)?;
    Ok(IndexDef {
        entity_kind,
        label_id,
        property_key_id,
        root_page_id,
    })
}
