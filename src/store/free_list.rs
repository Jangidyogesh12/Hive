use std::fs::OpenOptions;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::errors::DbError;

pub struct FreeList {
    freed: Vec<u64>,
    writer: BufWriter<std::fs::File>,
}

impl FreeList {
    /// Opens the free list file at `path`, loading any previously freed IDs.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let mut reader = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;
        let writer_file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;

        let len = reader
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::SeekError)?;

        let count = len / 8;
        let mut freed = Vec::with_capacity(count as usize);

        if count > 0 {
            reader
                .seek(SeekFrom::Start(0))
                .map_err(|_| DbError::SeekError)?;

            for _ in 0..count {
                let mut buf = [0u8; 8];
                reader
                    .read_exact(&mut buf)
                    .map_err(|_| DbError::ReadError)?;
                freed.push(u64::from_le_bytes(buf));
            }
        }

        Ok(Self {
            freed,
            writer: BufWriter::new(writer_file),
        })
    }

    /// Pops the most recently freed ID for reuse, flushing the list to disk.
    pub fn pop(&mut self) -> Option<u64> {
        let id = self.freed.pop()?;
        self.flush().ok()?;
        Some(id)
    }

    /// Returns the most recently freed ID without removing it.
    pub fn peek(&self) -> Option<u64> {
        self.freed.last().copied()
    }

    /// Returns a snapshot of the current free IDs in LIFO order.
    pub fn snapshot(&self) -> Vec<u64> {
        self.freed.clone()
    }

    /// Pushes a freed ID onto the list and flushes to disk.
    pub fn push(&mut self, id: u64) -> Result<(), DbError> {
        self.freed.push(id);
        self.flush()
    }

    /// Replaces the in-memory free list contents and flushes them to disk.
    pub fn replace(&mut self, ids: Vec<u64>) -> Result<(), DbError> {
        self.freed = ids;
        self.flush()
    }

    /// Writes the entire in-memory free list to disk, truncating the file first.
    pub fn flush(&mut self) -> Result<(), DbError> {
        self.writer.flush().map_err(|_| DbError::WriteError)?;
        self.writer
            .get_mut()
            .set_len(0)
            .map_err(|_| DbError::WriteError)?;

        self.writer
            .seek(SeekFrom::Start(0))
            .map_err(|_| DbError::SeekError)?;

        for id in &self.freed {
            self.writer
                .write_all(&id.to_le_bytes())
                .map_err(|_| DbError::WriteError)?;
        }
        self.writer.flush().map_err(|_| DbError::WriteError)?;
        Ok(())
    }

    /// Returns the number of freed IDs currently in the list.
    pub fn len(&self) -> usize {
        self.freed.len()
    }

    /// Returns true if there are no freed IDs available.
    pub fn is_empty(&self) -> bool {
        self.freed.is_empty()
    }

    pub fn persist(&mut self) -> Result<(), DbError> {
        self.flush()?;
        self.writer
            .get_ref()
            .sync_all()
            .map_err(DbError::Io)
    }
}
