use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::errors::DbError;

pub const HIVE_MAGIC: [u8; 8] = [b'H', b'I', b'V', b'E', 0, 0, 0, 1];
pub const CURRENT_VERSION: u32 = 1;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DbHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub node_count: u64,
    pub edge_count: u64,
    pub property_count: u64,
    pub free_node_head: u64,
    pub free_edge_head: u64,
}

impl DbHeader {
    pub const SIZE: usize = 52;

    /// Creates a new `DbHeader` with default (zero) counts and the current magic/version.
    pub fn new() -> Self {
        Self {
            magic: HIVE_MAGIC,
            version: CURRENT_VERSION,
            node_count: 0,
            edge_count: 0,
            property_count: 0,
            free_node_head: 0,
            free_edge_head: 0,
        }
    }

    /// Serializes the header into its 52-byte little-endian representation.
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..8].copy_from_slice(&self.magic);
        buf[8..12].copy_from_slice(&self.version.to_le_bytes());
        buf[12..20].copy_from_slice(&self.node_count.to_le_bytes());
        buf[20..28].copy_from_slice(&self.edge_count.to_le_bytes());
        buf[28..36].copy_from_slice(&self.property_count.to_le_bytes());
        buf[36..44].copy_from_slice(&self.free_node_head.to_le_bytes());
        buf[44..52].copy_from_slice(&self.free_edge_head.to_le_bytes());
        buf
    }

    /// Deserializes a header from its 52-byte little-endian representation.
    pub fn from_bytes(buf: [u8; Self::SIZE]) -> Self {
        Self {
            magic: buf[0..8].try_into().unwrap(),
            version: u32::from_le_bytes(buf[8..12].try_into().unwrap()),
            node_count: u64::from_le_bytes(buf[12..20].try_into().unwrap()),
            edge_count: u64::from_le_bytes(buf[20..28].try_into().unwrap()),
            property_count: u64::from_le_bytes(buf[28..36].try_into().unwrap()),
            free_node_head: u64::from_le_bytes(buf[36..44].try_into().unwrap()),
            free_edge_head: u64::from_le_bytes(buf[44..52].try_into().unwrap()),
        }
    }
}

/// Reads and validates the database header from the given meta file.
/// Returns an error if the file is too small or the magic bytes don't match.
pub fn read_header(path: &Path) -> Result<DbHeader, DbError> {
    let mut file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|_| DbError::FileOpenError)?;

    let len = file
        .seek(SeekFrom::End(0))
        .map_err(|_| DbError::SeekError)?;

    if (len as usize) < DbHeader::SIZE {
        return Err(DbError::InvalidHeader);
    }

    file.seek(SeekFrom::Start(0))
        .map_err(|_| DbError::SeekError)?;

    let mut buf = [0u8; DbHeader::SIZE];
    file.read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;

    let header = DbHeader::from_bytes(buf);
    if header.magic != HIVE_MAGIC {
        return Err(DbError::InvalidHeader);
    }
    Ok(header)
}

/// Writes a `DbHeader` to the given meta file, creating it if necessary.
pub fn write_header(path: &Path, header: DbHeader) -> Result<(), DbError> {
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| DbError::FileOpenError)?;

    let buf = header.to_bytes();
    file.seek(SeekFrom::Start(0))
        .map_err(|_| DbError::SeekError)?;
    file.write_all(&buf)
        .map_err(|_| DbError::WriteError)?;
    file.flush().map_err(|_| DbError::WriteError)?;
    Ok(())
}
