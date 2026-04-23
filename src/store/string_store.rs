use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::errors::DbError;

pub struct StringStore {
    file: File,
}

impl StringStore {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;

        Ok(Self { file })
    }

    pub fn append(&mut self, s: &str) -> Result<u64, DbError> {
        let bytes = s.as_bytes();
        let offset = self
            .file
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::WriteError)?;

        let len = bytes.len() as u32;

        self.file
            .write_all(&len.to_le_bytes())
            .map_err(|_| DbError::SeekError)?;
        self.file
            .write_all(&bytes)
            .map_err(|_| DbError::WriteError)?;

        self.file.flush().map_err(|_| DbError::WriteError)?;

        return Ok(offset);
    }

    pub fn read(&mut self, offset: u64) -> Result<String, DbError> {
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::ReadError)?;

        let mut len_buf = [0u8; 4];

        self.file
            .read_exact(&mut len_buf)
            .map_err(|_| DbError::ReadError)?;

        let len = u32::from_le_bytes(len_buf);

        let mut str_buf = vec![0u8; len as usize];
        self.file
            .read_exact(&mut str_buf)
            .map_err(|_| DbError::ReadError)?;

        return String::from_utf8(str_buf).map_err(|_| DbError::ReadError);
    }
}
