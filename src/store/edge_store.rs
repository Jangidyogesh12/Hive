// File-backed storage API for fixed-size edge records.
use crate::store::edge_record::EdgeRecord;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::errors::DbError;

// Append/read/update operations over an edge record file.
pub struct EdgeStore {
    file: std::fs::File,
}

impl EdgeStore {
    // Opens an edge store file, creating it if it does not exist.
    pub fn open(path: &std::path::Path) -> Result<Self, DbError> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;

        Ok(Self { file })
    }

    // Appends an edge record at the end of the file.
    pub fn append(&mut self, record: EdgeRecord) -> Result<(), DbError> {
        let buf = record.to_bytes();

        self.file
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::SeekError)?;

        self.file.write_all(&buf).map_err(|_| DbError::WriteError)?;

        self.file.flush().map_err(|_| DbError::WriteError)?;

        Ok(())
    }

    // Reads an edge record by zero-based record index.
    pub fn read(&mut self, idx: u64) -> Result<EdgeRecord, DbError> {
        let offset = idx * EdgeRecord::SIZE as u64;

        let mut buf = [0u8; EdgeRecord::SIZE];

        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::ReadError)?;
        self.file
            .read_exact(&mut buf)
            .map_err(|_| DbError::ReadError)?;

        Ok(EdgeRecord::from_bytes(buf))
    }

    // Updates an edge record at the given zero-based record index.
    pub fn update(&mut self, idx: u64, record: EdgeRecord) -> Result<(), DbError> {
        let offset = idx * EdgeRecord::SIZE as u64;

        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::SeekError)?;

        let buf = record.to_bytes();

        self.file.write_all(&buf).map_err(|_| DbError::WriteError)?;

        self.file.flush().map_err(|_| DbError::WriteError)?;

        Ok(())
    }
}
