//! Canonical key encoding for B-tree entries.
//!
//! Keys are encoded into a single byte slice with a total order so that
//! binary search and range scans on the encoded form match the logical
//! ordering of the key values.

use crate::errors::DbError;
use std::cmp::Ordering;

/// A typed key that can be stored in a B-tree index.
///
/// Only a subset of Hive value types are supported as index keys.  Composite
/// keys are used for multi-column indexes (e.g. label + property value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtreeKey {
    /// SQL-style NULL, sorted before all non-null values.
    Null,
    /// Signed integer encoded so that numeric order matches byte order.
    Int(i64),
    /// UTF-8 text value.
    Text(String),
    /// Raw byte string.
    Bytes(Vec<u8>),
    /// Ordered sequence of sub-keys for composite indexes.
    Composite(Vec<BtreeKey>),
}

impl BtreeKey {
    /// Type-tag byte written at the start of an encoded key.
    pub const TAG_NULL: u8 = 0x01;
    pub const TAG_INT: u8 = 0x02;
    pub const TAG_TEXT: u8 = 0x03;
    pub const TAG_BYTES: u8 = 0x04;
    pub const TAG_COMPOSITE: u8 = 0x05;

    /// Encodes this key into `out`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            BtreeKey::Null => out.push(Self::TAG_NULL),
            BtreeKey::Int(v) => {
                out.push(Self::TAG_INT);
                // Flip the sign bit so that two's complement bytes sort
                // in the same order as the signed numeric values.
                let bits = (*v as u64) ^ 0x8000_0000_0000_0000;
                out.extend_from_slice(&bits.to_be_bytes());
            }
            BtreeKey::Text(s) => {
                out.push(Self::TAG_TEXT);
                let bytes = s.as_bytes();
                out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                out.extend_from_slice(bytes);
            }
            BtreeKey::Bytes(b) => {
                out.push(Self::TAG_BYTES);
                out.extend_from_slice(&(b.len() as u32).to_be_bytes());
                out.extend_from_slice(b);
            }
            BtreeKey::Composite(parts) => {
                out.push(Self::TAG_COMPOSITE);
                out.extend_from_slice(&(parts.len() as u32).to_be_bytes());
                for part in parts {
                    part.encode(out);
                }
            }
        }
    }

    /// Returns the encoded form of this key as a new vector.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode(&mut out);
        out
    }

    fn decode_one(buf: &[u8]) -> Result<(Self, &[u8]), DbError> {
        if buf.is_empty() {
            return Err(DbError::ReadError);
        }
        let tag = buf[0];
        let rest = &buf[1..];
        match tag {
            Self::TAG_NULL => Ok((BtreeKey::Null, rest)),
            Self::TAG_INT => {
                if rest.len() < 8 {
                    return Err(DbError::ReadError);
                }
                let bits = u64::from_be_bytes(rest[0..8].try_into().unwrap());
                let v = (bits ^ 0x8000_0000_0000_0000) as i64;
                Ok((BtreeKey::Int(v), &rest[8..]))
            }
            Self::TAG_TEXT | Self::TAG_BYTES => {
                if rest.len() < 4 {
                    return Err(DbError::ReadError);
                }
                let len = u32::from_be_bytes(rest[0..4].try_into().unwrap()) as usize;
                let payload = &rest[4..];
                if payload.len() < len {
                    return Err(DbError::ReadError);
                }
                let value = payload[..len].to_vec();
                let remaining = &payload[len..];
                if tag == Self::TAG_TEXT {
                    let s = String::from_utf8(value).map_err(|_| DbError::ReadError)?;
                    Ok((BtreeKey::Text(s), remaining))
                } else {
                    Ok((BtreeKey::Bytes(value), remaining))
                }
            }
            Self::TAG_COMPOSITE => {
                if rest.len() < 4 {
                    return Err(DbError::ReadError);
                }
                let count = u32::from_be_bytes(rest[0..4].try_into().unwrap()) as usize;
                let mut remaining = &rest[4..];
                let mut parts = Vec::with_capacity(count);
                for _ in 0..count {
                    let (part, next) = Self::decode_one(remaining)?;
                    parts.push(part);
                    remaining = next;
                }
                Ok((BtreeKey::Composite(parts), remaining))
            }
            _ => Err(DbError::ReadError),
        }
    }

    /// Decodes a key previously produced by [`encode`](Self::encode).
    pub fn decode(buf: &[u8]) -> Result<Self, DbError> {
        let (key, _rest) = Self::decode_one(buf)?;
        Ok(key)
    }
}

/// Compares two encoded keys and returns their ordering.
///
/// This is a lexicographic comparison over the canonical byte encoding.
pub fn cmp_encoded(a: &[u8], b: &[u8]) -> Ordering {
    a.cmp(b)
}
