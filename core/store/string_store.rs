use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::errors::DbError;

pub struct StringStore {
    reader: File,
    writer: BufWriter<File>,
}

impl StringStore {
    /// Opens the string store file at `path`, creating it if it does not exist.
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

    /// Appends a length-prefixed string to the end of the file.
    /// Returns the file offset where the string was written.
    pub fn append(&mut self, s: &str) -> Result<u64, DbError> {
        let bytes = s.as_bytes();
        self.writer.flush().map_err(|_| DbError::WriteError)?;
        let offset = self
            .writer
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::WriteError)?;

        let len = bytes.len() as u32;

        self.writer
            .write_all(&len.to_le_bytes())
            .map_err(|_| DbError::SeekError)?;
        self.writer
            .write_all(bytes)
            .map_err(|_| DbError::WriteError)?;

        Ok(offset)
    }

    /// Reads a length-prefixed string from the given file offset.
    pub fn read(&mut self, offset: u64) -> Result<String, DbError> {
        self.flush()?;

        self.reader
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::ReadError)?;

        let mut len_buf = [0u8; 4];

        self.reader
            .read_exact(&mut len_buf)
            .map_err(|_| DbError::ReadError)?;

        let len = u32::from_le_bytes(len_buf);

        let mut str_buf = vec![0u8; len as usize];
        self.reader
            .read_exact(&mut str_buf)
            .map_err(|_| DbError::ReadError)?;

        String::from_utf8(str_buf).map_err(|_| DbError::ReadError)
    }

    pub fn flush(&mut self) -> Result<(), DbError> {
        self.writer.flush().map_err(|_| DbError::WriteError)
    }

    pub fn sync(&mut self) -> Result<(), DbError> {
        self.flush()?;
        self.writer.get_ref().sync_all().map_err(DbError::Io)
    }
}
