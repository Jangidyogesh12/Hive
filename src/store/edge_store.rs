use crate::store::edge_record::EdgeRecord;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::errors::DbError;
pub struct EdgeStore {
    file: std::fs::File,
}

impl EdgeStore {
    pub fn open(path: &std::path::Path) -> Result<Self, DbError> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;

        Ok(Self { file })
    }

    pub fn append(&mut self, record: EdgeRecord) -> Result<(), DbError> {
        let buf = record.to_bytes();

        self.file
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::SeekError)?;

        self.file.write_all(&buf).map_err(|_| DbError::WriteError)?;

        self.file.flush().map_err(|_| DbError::WriteError)?;

        Ok(())
    }

    pub fn read(&mut self, id: u64) -> Result<EdgeRecord, DbError> {
        let offset = id * EdgeRecord::SIZE as u64;

        let mut buf = [0u8; EdgeRecord::SIZE];

        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::ReadError)?;
        self.file
            .read_exact(&mut buf)
            .map_err(|_| DbError::ReadError)?;

        Ok(EdgeRecord::from_bytes(buf))
    }

    pub fn update(&mut self, id: u64, record: EdgeRecord) -> Result<(), DbError> {
        let offset = id * EdgeRecord::SIZE as u64;

        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::SeekError)?;

        let buf = record.to_bytes();

        self.file.write_all(&buf).map_err(|_| DbError::WriteError)?;

        self.file.flush().map_err(|_| DbError::WriteError)?;

        Ok(())
    }
}
