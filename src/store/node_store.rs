// File-backed storage API for fixed-size node records.
use crate::store::node_record::NodeRecord;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::errors::DbError;

// Append/read/update operations over a node record file.
pub struct NodeStore {
    file: std::fs::File,
}

impl NodeStore {
    // Opens a node store file, creating it if it does not exist.
    pub fn open(path: &std::path::Path) -> Result<Self, DbError> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;

        Ok(Self { file })
    }

    // Appends a node record at the end of the file.
    pub fn append(&mut self, record: NodeRecord) -> Result<(), DbError> {
        let buf = record.to_bytes();

        self.file
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::SeekError)?;

        self.file.write_all(&buf).map_err(|_| DbError::WriteError)?;

        self.file.flush().map_err(|_| DbError::WriteError)?;

        Ok(())
    }

    // Reads a node record by zero-based record index.
    pub fn read(&mut self, idx: u64) -> Result<NodeRecord, DbError> {
        let offset = idx * NodeRecord::SIZE as u64;

        let mut buf = [0u8; NodeRecord::SIZE];

        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::ReadError)?;
        self.file
            .read_exact(&mut buf)
            .map_err(|_| DbError::ReadError)?;

        Ok(NodeRecord::from_bytes(buf))
    }

    // Updates a node record at the given zero-based record index.
    pub fn update(&mut self, idx: u64, record: NodeRecord) -> Result<(), DbError> {
        let offset = idx * NodeRecord::SIZE as u64;

        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::SeekError)?;

        let buf = record.to_bytes();

        self.file.write_all(&buf).map_err(|_| DbError::WriteError)?;

        self.file.flush().map_err(|_| DbError::WriteError)?;

        Ok(())
    }
}
