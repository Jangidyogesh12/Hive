// File-backed storage API for fixed-size property records.
use crate::store::property::record::PropertyRecord;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

use crate::errors::DbError;

// Append/read/update operations over a property record file.
pub struct PropertyStore {
    file: File,
}

impl PropertyStore {
    /// Opens the property store file at `path`, creating it if it does not exist.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;

        Ok(Self { file })
    }

    /// Appends a property record at the end of the file.
    pub fn append(&mut self, record: PropertyRecord) -> Result<(), DbError> {
        let buf = record.to_bytes();

        self.file
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::SeekError)?;

        self.file.write_all(&buf).map_err(|_| DbError::WriteError)?;

        self.file.flush().map_err(|_| DbError::WriteError)?;

        Ok(())
    }

    /// Reads a property record by its zero-based record index.
    pub fn read(&mut self, idx: u64) -> Result<PropertyRecord, DbError> {
        let offset = idx * PropertyRecord::SIZE as u64;

        let mut buf = [0u8; PropertyRecord::SIZE];

        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::ReadError)?;
        self.file
            .read_exact(&mut buf)
            .map_err(|_| DbError::ReadError)?;

        Ok(PropertyRecord::from_bytes(buf))
    }

    /// Updates (overwrites) a property record at the given zero-based index.
    pub fn update(&mut self, idx: u64, record: PropertyRecord) -> Result<(), DbError> {
        let offset = idx * PropertyRecord::SIZE as u64;

        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::SeekError)?;

        let buf = record.to_bytes();

        self.file.write_all(&buf).map_err(|_| DbError::WriteError)?;

        self.file.flush().map_err(|_| DbError::WriteError)?;

        Ok(())
    }

    /// Returns the total number of property records in the file.
    pub fn count(&mut self) -> Result<u64, DbError> {
        let len = self
            .file
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::SeekError)?;

        Ok(len / PropertyRecord::SIZE as u64)
    }
}
