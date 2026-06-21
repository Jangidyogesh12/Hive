// File-backed storage API for fixed-size node records.
use crate::store::node::record::NodeRecord;
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
};

use crate::errors::DbError;

// Append/read/update operations over a node record file.
pub struct NodeStore {
    reader: File,
    writer: BufWriter<File>,
}

impl NodeStore {
    /// Opens the node store file at `path`, creating it if it does not exist.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let reader = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;
        let writer_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;

        Ok(Self {
            reader,
            writer: BufWriter::new(writer_file),
        })
    }

    /// Appends a node record at the end of the file.
    pub fn append(&mut self, record: NodeRecord) -> Result<(), DbError> {
        let buf = record.to_bytes();

        self.writer
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::SeekError)?;

        self.writer
            .write_all(&buf)
            .map_err(|_| DbError::WriteError)?;

        Ok(())
    }

    /// Reads a node record by its zero-based record index.
    pub fn read(&mut self, idx: u64) -> Result<NodeRecord, DbError> {
        self.flush()?;

        let offset = idx * NodeRecord::SIZE as u64;

        let mut buf = [0u8; NodeRecord::SIZE];

        self.reader
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::ReadError)?;
        self.reader
            .read_exact(&mut buf)
            .map_err(|_| DbError::ReadError)?;

        Ok(NodeRecord::from_bytes(buf))
    }

    /// Updates (overwrites) a node record at the given zero-based index.
    pub fn update(&mut self, idx: u64, record: NodeRecord) -> Result<(), DbError> {
        let offset = idx * NodeRecord::SIZE as u64;

        self.writer
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::SeekError)?;

        let buf = record.to_bytes();

        self.writer
            .write_all(&buf)
            .map_err(|_| DbError::WriteError)?;

        Ok(())
    }

    /// Returns the total number of node records in the file.
    pub fn count(&mut self) -> Result<u64, DbError> {
        self.flush()?;

        let len = self
            .reader
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::SeekError)?;

        Ok(len / NodeRecord::SIZE as u64)
    }

    pub fn flush(&mut self) -> Result<(), DbError> {
        self.writer.flush().map_err(|_| DbError::WriteError)
    }

    pub fn sync(&mut self) -> Result<(), DbError> {
        self.flush()?;
        self.writer.get_ref().sync_all().map_err(DbError::Io)
    }
}
