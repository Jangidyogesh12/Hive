/// Variable-width record layouts for nodes, edges, and properties within slotted pages.
use super::serializer;
use crate::errors::DbError;
use crate::types::NIL_ID;
use crate::value;

/// On-disk representation of a single node stored in a slotted page.
///
/// Serialized layout (fixed prefix):
/// `[flags: u8][label_id: u32][id: u64][first_out_edge: u64][first_in_edge: u64][first_property: u64]`
///
/// Followed by a `[prop_count: u16]` and then `prop_count` inline property entries.
pub struct NodeRecord {
    /// Logical node ID (monotonically increasing, assigned at creation).
    pub id: u64,
    /// ID of the label associated with this node (0 = unlabeled).
    pub label_id: u32,
    /// Reserved flags for future use.
    pub flags: u32,
    /// Packed record ID of the first outgoing edge, or `NIL_ID` if none.
    pub first_out_edge: u64,
    /// Packed record ID of the first incoming edge, or `NIL_ID` if none.
    pub first_in_edge: u64,
    /// Reserved for future linked-list property storage (currently unused).
    pub first_property: u64,
    /// Inline property entries stored directly in the node record.
    pub properties: Vec<PropertyEntry>,
}

/// On-disk representation of a single directed edge stored in a slotted page.
///
/// Serialized layout (fixed prefix):
/// `[flags: u8][label_id: u32][id: u64][src: u64][dst: u64][next_out_edge: u64][next_in_edge: u64][first_property: u64]`
///
/// Followed by a `[prop_count: u16]` and then `prop_count` inline property entries.
pub struct EdgeRecord {
    /// Logical edge ID (monotonically increasing, assigned at creation).
    pub id: u64,
    /// ID of the label/type associated with this edge (0 = unlabeled).
    pub label_id: u32,
    /// Reserved flags for future use.
    pub flags: u32,
    /// Packed record ID of the source node.
    pub src: u64,
    /// Packed record ID of the destination node.
    pub dst: u64,
    /// Packed record ID of the next incoming edge to `dst`, or `NIL_ID` if none.
    pub next_in_edge: u64,
    /// Packed record ID of the next outgoing edge from `src`, or `NIL_ID` if none.
    pub next_out_edge: u64,
    /// Reserved for future linked-list property storage (currently unused).
    pub first_property: u64,
    /// Inline property entries stored directly in the edge record.
    pub properties: Vec<PropertyEntry>,
}

/// Fixed-size on-disk record for a single property value (reserved for future use).
///
/// This record type is defined for potential future external property storage
/// where properties are moved out of node/edge records into separate pages.
/// Currently unused — properties are stored inline via `PropertyEntry`.
pub struct PropertyRecord {
    /// Logical property ID.
    pub id: u64,
    /// ID of the property key in the property-key dictionary.
    pub key_id: u32,
    /// Byte offset to the key name in the dictionary page, or `NIL_ID` if unused.
    pub key_offset: u64,
    /// Type tag identifying the value encoding (see `value` module constants).
    pub value_type: u8,
    /// Inline value bytes (up to 15 bytes stored directly).
    pub value_inline: [u8; 15],
    /// Packed record ID of the next property in the chain, or `NIL_ID` if none.
    pub next_property: u64,
    /// Reserved flags for future use.
    pub flags: u32,
    /// Reserved field for future use.
    pub reserved: u32,
}

/// Compact property entry stored inline within a node or edge record.
///
/// Each entry stores a `key_id` referencing the property-key dictionary,
/// a type tag, up to 15 inline value bytes, and an overflow pointer for
/// long strings.  Multiple entries are serialized sequentially after the
/// node/edge fixed prefix.
pub struct PropertyEntry {
    /// ID of the property key in the property-key dictionary.
    pub key_id: u32,
    /// Type tag identifying the value encoding (see `value` module constants).
    pub value_type: u8,
    /// Inline value bytes (up to 15 bytes stored directly).
    pub value_inline: [u8; 15],
    /// Page offset to overflow data for long strings, or 0 if unused.
    pub long_value_offset: u64,
}

/// Fixed prefix size of a serialized node record (excluding properties): 1 + 4 + 8 + 8 + 8 + 8.
const NODE_FIXED_PREFIX: usize = 39;
/// Fixed prefix size of a serialized edge record (excluding properties): 1 + 4 + 8 + 8 + 8 + 8 + 8 + 8.
const EDGE_FIXED_PREFIX: usize = 63;
/// Base size of a serialized property entry (key_id + reserved + value_type + value_inline).
const PROPERTY_ENTRY_BASE_SIZE: usize = 25;

impl NodeRecord {
    /// Creates an empty node record with no label, edges, or properties yet.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            label_id: 0,
            flags: 0,
            first_out_edge: NIL_ID,
            first_in_edge: NIL_ID,
            first_property: NIL_ID,
            properties: Vec::new(),
        }
    }

    /// Returns the exact number of bytes needed to serialize this node record.
    pub fn encoded_size(&self) -> usize {
        let props_size: usize = self
            .properties
            .iter()
            .map(|p| PROPERTY_ENTRY_BASE_SIZE + self.property_value_size(p))
            .sum();
        NODE_FIXED_PREFIX + props_size
    }

    /// Returns extra bytes needed by a property entry's non-inline value.
    fn property_value_size(&self, entry: &PropertyEntry) -> usize {
        match entry.value_type {
            value::LONG_STRING => serializer::var_int_size(entry.long_value_offset),
            _ => 0,
        }
    }

    /// Serializes this node record into the provided output buffer.
    pub fn to_bytes(&self, buf: &mut [u8]) -> Result<usize, DbError> {
        let size = self.encoded_size();
        if buf.len() < size {
            return Err(DbError::WriteError);
        }
        let mut pos = 0;
        serializer::put_u8(buf, pos, self.flags as u8);
        pos += 1;
        serializer::put_u32_le(buf, pos, self.label_id);
        pos += 4;
        serializer::put_u64_le(buf, pos, self.id);
        pos += 8;
        serializer::put_u64_le(buf, pos, self.first_out_edge);
        pos += 8;
        serializer::put_u64_le(buf, pos, self.first_in_edge);
        pos += 8;
        serializer::put_u64_le(buf, pos, self.first_property);
        pos += 8;

        let prop_count = self.properties.len() as u16;
        serializer::put_u16_le(buf, pos, prop_count);
        pos += 2;

        for entry in &self.properties {
            serializer::put_u32_le(buf, pos, entry.key_id);
            pos += 4;
            serializer::put_u32_le(buf, pos, 0);
            pos += 4;
            serializer::put_u8(buf, pos, entry.value_type);
            pos += 1;
            buf[pos..pos + 15].copy_from_slice(&entry.value_inline);
            pos += 15;

            if entry.value_type == value::LONG_STRING {
                pos += serializer::var_int_write(&mut buf[pos..], entry.long_value_offset);
            }
        }

        Ok(size)
    }

    /// Deserializes a node record from bytes read from a page slot.
    pub fn from_bytes(buf: &[u8]) -> Result<Self, DbError> {
        if buf.len() < NODE_FIXED_PREFIX {
            return Err(DbError::ReadError);
        }
        let mut pos = 0;
        let flags = serializer::get_u8(buf, pos) as u32;
        pos += 1;
        let label_id = serializer::get_u32_le(buf, pos);
        pos += 4;
        let id = serializer::get_u64_le(buf, pos);
        pos += 8;
        let first_out_edge = serializer::get_u64_le(buf, pos);
        pos += 8;
        let first_in_edge = serializer::get_u64_le(buf, pos);
        pos += 8;
        let first_property = serializer::get_u64_le(buf, pos);
        pos += 8;

        let prop_count = serializer::get_u16_le(buf, pos);
        pos += 2;

        let mut properties = Vec::with_capacity(prop_count as usize);
        for _ in 0..prop_count {
            if pos + PROPERTY_ENTRY_BASE_SIZE > buf.len() {
                return Err(DbError::ReadError);
            }
            let key_id = serializer::get_u32_le(buf, pos);
            pos += 8;
            let value_type = serializer::get_u8(buf, pos);
            pos += 1;
            let mut value_inline = [0u8; 15];
            value_inline.copy_from_slice(&buf[pos..pos + 15]);
            pos += 15;

            let long_value_offset = if value_type == value::LONG_STRING {
                let (off, read) = serializer::var_int_read(&buf[pos..])?;
                pos += read;
                off
            } else {
                0
            };

            properties.push(PropertyEntry {
                key_id,
                value_type,
                value_inline,
                long_value_offset,
            });
        }

        Ok(Self {
            id,
            label_id,
            flags,
            first_out_edge,
            first_in_edge,
            first_property,
            properties,
        })
    }
}

impl EdgeRecord {
    /// Creates an empty edge record whose endpoints and chain links are unset.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            label_id: 0,
            flags: 0,
            src: NIL_ID,
            dst: NIL_ID,
            next_out_edge: NIL_ID,
            next_in_edge: NIL_ID,
            first_property: NIL_ID,
            properties: Vec::new(),
        }
    }

    /// Returns the exact number of bytes needed to serialize this edge record.
    pub fn encoded_size(&self) -> usize {
        let props_size: usize = self
            .properties
            .iter()
            .map(|p| PROPERTY_ENTRY_BASE_SIZE + self.property_value_size(p))
            .sum();
        EDGE_FIXED_PREFIX + props_size
    }

    /// Returns extra bytes needed by a property entry's non-inline value.
    fn property_value_size(&self, entry: &PropertyEntry) -> usize {
        match entry.value_type {
            value::LONG_STRING => serializer::var_int_size(entry.long_value_offset),
            _ => 0,
        }
    }

    /// Serializes this edge record into the provided output buffer.
    pub fn to_bytes(&self, buf: &mut [u8]) -> Result<usize, DbError> {
        let size = self.encoded_size();
        if buf.len() < size {
            return Err(DbError::WriteError);
        }
        let mut pos = 0;
        serializer::put_u8(buf, pos, self.flags as u8);
        pos += 1;
        serializer::put_u32_le(buf, pos, self.label_id);
        pos += 4;
        serializer::put_u64_le(buf, pos, self.id);
        pos += 8;
        serializer::put_u64_le(buf, pos, self.src);
        pos += 8;
        serializer::put_u64_le(buf, pos, self.dst);
        pos += 8;
        serializer::put_u64_le(buf, pos, self.next_out_edge);
        pos += 8;
        serializer::put_u64_le(buf, pos, self.next_in_edge);
        pos += 8;
        serializer::put_u64_le(buf, pos, self.first_property);
        pos += 8;

        let prop_count = self.properties.len() as u16;
        serializer::put_u16_le(buf, pos, prop_count);
        pos += 2;

        for entry in &self.properties {
            serializer::put_u32_le(buf, pos, entry.key_id);
            pos += 4;
            serializer::put_u32_le(buf, pos, 0);
            pos += 4;
            serializer::put_u8(buf, pos, entry.value_type);
            pos += 1;
            buf[pos..pos + 15].copy_from_slice(&entry.value_inline);
            pos += 15;

            if entry.value_type == value::LONG_STRING {
                pos += serializer::var_int_write(&mut buf[pos..], entry.long_value_offset);
            }
        }

        Ok(size)
    }

    /// Deserializes an edge record from bytes read from a page slot.
    pub fn from_bytes(buf: &[u8]) -> Result<Self, DbError> {
        if buf.len() < EDGE_FIXED_PREFIX {
            return Err(DbError::ReadError);
        }
        let mut pos = 0;
        let flags = serializer::get_u8(buf, pos) as u32;
        pos += 1;
        let label_id = serializer::get_u32_le(buf, pos);
        pos += 4;
        let id = serializer::get_u64_le(buf, pos);
        pos += 8;
        let src = serializer::get_u64_le(buf, pos);
        pos += 8;
        let dst = serializer::get_u64_le(buf, pos);
        pos += 8;
        let next_out_edge = serializer::get_u64_le(buf, pos);
        pos += 8;
        let next_in_edge = serializer::get_u64_le(buf, pos);
        pos += 8;
        let first_property = serializer::get_u64_le(buf, pos);
        pos += 8;

        let prop_count = serializer::get_u16_le(buf, pos);
        pos += 2;

        let mut properties = Vec::with_capacity(prop_count as usize);
        for _ in 0..prop_count {
            if pos + PROPERTY_ENTRY_BASE_SIZE > buf.len() {
                return Err(DbError::ReadError);
            }
            let key_id = serializer::get_u32_le(buf, pos);
            pos += 8;
            let value_type = serializer::get_u8(buf, pos);
            pos += 1;
            let mut value_inline = [0u8; 15];
            value_inline.copy_from_slice(&buf[pos..pos + 15]);
            pos += 15;

            let long_value_offset = if value_type == value::LONG_STRING {
                let (off, read) = serializer::var_int_read(&buf[pos..])?;
                pos += read;
                off
            } else {
                0
            };

            properties.push(PropertyEntry {
                key_id,
                value_type,
                value_inline,
                long_value_offset,
            });
        }

        Ok(Self {
            id,
            label_id,
            flags,
            src,
            dst,
            next_out_edge,
            next_in_edge,
            first_property,
            properties,
        })
    }
}

impl PropertyRecord {
    pub const SIZE: usize = 56;

    /// Creates an empty property record with unset key/value links.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            key_id: 0,
            key_offset: NIL_ID,
            value_type: 0,
            value_inline: [0; 15],
            next_property: NIL_ID,
            flags: 0,
            reserved: 0,
        }
    }

    /// Returns the fixed serialized size of a property record.
    pub fn encoded_size(&self) -> usize {
        Self::SIZE
    }

    /// Serializes this property record into the provided output buffer.
    pub fn to_bytes(&self, buf: &mut [u8]) -> Result<usize, DbError> {
        if buf.len() < Self::SIZE {
            return Err(DbError::WriteError);
        }
        buf[0..Self::SIZE].fill(0);
        serializer::put_u64_le(buf, 0, self.id);
        serializer::put_u32_le(buf, 8, self.key_id);
        serializer::put_u32_le(buf, 12, 0);
        serializer::put_u64_le(buf, 16, self.key_offset);
        serializer::put_u8(buf, 24, self.value_type);
        buf[25..40].copy_from_slice(&self.value_inline);
        serializer::put_u64_le(buf, 40, self.next_property);
        serializer::put_u32_le(buf, 48, self.flags);
        serializer::put_u32_le(buf, 52, self.reserved);
        Ok(Self::SIZE)
    }

    /// Deserializes a property record from bytes read from a page slot.
    pub fn from_bytes(buf: &[u8]) -> Result<Self, DbError> {
        if buf.len() < Self::SIZE {
            return Err(DbError::ReadError);
        }
        Ok(Self {
            id: serializer::get_u64_le(buf, 0),
            key_id: serializer::get_u32_le(buf, 8),
            key_offset: serializer::get_u64_le(buf, 16),
            value_type: serializer::get_u8(buf, 24),
            value_inline: buf[25..40].try_into().unwrap(),
            next_property: serializer::get_u64_le(buf, 40),
            flags: serializer::get_u32_le(buf, 48),
            reserved: serializer::get_u32_le(buf, 52),
        })
    }
}

/// A slot index within a page. Valid only within the page that created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotIndex(pub u16);
