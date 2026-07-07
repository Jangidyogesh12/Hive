/// Variable-width record layouts for nodes, edges, and properties within slotted pages.
use super::serializer;
use crate::errors::DbError;
use crate::types::NIL_ID;
use crate::value;

pub struct NodeRecordV2 {
    pub id: u64,
    pub label_id: u32,
    pub flags: u32,
    pub first_out_edge: u64,
    pub first_in_edge: u64,
    pub first_property: u64,
    pub properties: Vec<PropertyEntry>,
}

pub struct EdgeRecordV2 {
    pub id: u64,
    pub label_id: u32,
    pub flags: u32,
    pub src: u64,
    pub dst: u64,
    pub next_out_edge: u64,
    pub next_in_edge: u64,
    pub first_property: u64,
    pub properties: Vec<PropertyEntry>,
}

pub struct PropertyRecordV2 {
    pub id: u64,
    pub key_hash: u64,
    pub key_offset: u64,
    pub value_type: u8,
    pub value_inline: [u8; 15],
    pub next_property: u64,
    pub flags: u32,
    pub reserved: u32,
}

pub struct PropertyEntry {
    pub key_hash: u64,
    pub value_type: u8,
    pub value_inline: [u8; 15],
    pub long_value_offset: u64,
}

const NODE_FIXED_PREFIX: usize = 39;
const EDGE_FIXED_PREFIX: usize = 63;
const PROPERTY_ENTRY_BASE_SIZE: usize = 25;

impl NodeRecordV2 {
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

    pub fn encoded_size(&self) -> usize {
        let props_size: usize = self
            .properties
            .iter()
            .map(|p| PROPERTY_ENTRY_BASE_SIZE + self.property_value_size(p))
            .sum();
        NODE_FIXED_PREFIX + props_size
    }

    fn property_value_size(&self, entry: &PropertyEntry) -> usize {
        match entry.value_type {
            value::LONG_STRING => serializer::var_int_size(entry.long_value_offset),
            _ => 0,
        }
    }

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
            serializer::put_u64_le(buf, pos, entry.key_hash);
            pos += 8;
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
            let key_hash = serializer::get_u64_le(buf, pos);
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
                key_hash,
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

impl EdgeRecordV2 {
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

    pub fn encoded_size(&self) -> usize {
        let props_size: usize = self
            .properties
            .iter()
            .map(|p| PROPERTY_ENTRY_BASE_SIZE + self.property_value_size(p))
            .sum();
        EDGE_FIXED_PREFIX + props_size
    }

    fn property_value_size(&self, entry: &PropertyEntry) -> usize {
        match entry.value_type {
            value::LONG_STRING => serializer::var_int_size(entry.long_value_offset),
            _ => 0,
        }
    }

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
            serializer::put_u64_le(buf, pos, entry.key_hash);
            pos += 8;
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
            let key_hash = serializer::get_u64_le(buf, pos);
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
                key_hash,
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

impl PropertyRecordV2 {
    pub const SIZE: usize = 56;

    pub fn new(id: u64) -> Self {
        Self {
            id,
            key_hash: NIL_ID,
            key_offset: NIL_ID,
            value_type: 0,
            value_inline: [0; 15],
            next_property: NIL_ID,
            flags: 0,
            reserved: 0,
        }
    }

    pub fn encoded_size(&self) -> usize {
        Self::SIZE
    }

    pub fn to_bytes(&self, buf: &mut [u8]) -> Result<usize, DbError> {
        if buf.len() < Self::SIZE {
            return Err(DbError::WriteError);
        }
        buf[0..Self::SIZE].fill(0);
        serializer::put_u64_le(buf, 0, self.id);
        serializer::put_u64_le(buf, 8, self.key_hash);
        serializer::put_u64_le(buf, 16, self.key_offset);
        serializer::put_u8(buf, 24, self.value_type);
        buf[25..40].copy_from_slice(&self.value_inline);
        serializer::put_u64_le(buf, 40, self.next_property);
        serializer::put_u32_le(buf, 48, self.flags);
        serializer::put_u32_le(buf, 52, self.reserved);
        Ok(Self::SIZE)
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, DbError> {
        if buf.len() < Self::SIZE {
            return Err(DbError::ReadError);
        }
        Ok(Self {
            id: serializer::get_u64_le(buf, 0),
            key_hash: serializer::get_u64_le(buf, 8),
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
