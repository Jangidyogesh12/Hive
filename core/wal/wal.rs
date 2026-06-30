use crate::errors::DbError;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::Path;
use super::utils::crc32_for_entry;

use super::wal_entry::WalEntry;

pub struct Wal {
    reader: File,
    writer: BufWriter<File>,
}

impl Wal {
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

    pub fn append(&mut self, entry: &WalEntry) -> Result<(), DbError> {
        let entry_type = entry.entry_type() as u8;
        let payload = entry.encode_payload()?;
        let checksum = crc32_for_entry(entry_type, &payload);
        let length = (1 + payload.len() + 4) as u32;

        self.writer
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::SeekError)?;
        self.writer
            .write_all(&length.to_le_bytes())
            .map_err(|_| DbError::WriteError)?;
        self.writer
            .write_all(&[entry_type])
            .map_err(|_| DbError::WriteError)?;
        self.writer
            .write_all(&payload)
            .map_err(|_| DbError::WriteError)?;
        self.writer
            .write_all(&checksum.to_le_bytes())
            .map_err(|_| DbError::WriteError)?;
        self.writer.flush().map_err(|_| DbError::WriteError)?;

        Ok(())
    }

    pub fn read_all(&mut self) -> Result<Vec<WalEntry>, DbError> {
        self.flush()?;

        self.reader
            .seek(SeekFrom::Start(0))
            .map_err(|_| DbError::SeekError)?;

        let mut entries = Vec::new();

        loop {
            let mut len_buf = [0u8; 4];

            match self.reader.read_exact(&mut len_buf) {
                Ok(()) => {}
                Err(err) if err.kind() == ErrorKind::UnexpectedEof => break,
                Err(_) => return Err(DbError::ReadError),
            }

            let length = u32::from_le_bytes(len_buf) as usize;
            if length < 5 {
                break;
            }

            let mut body = vec![0u8; length];
            match self.reader.read_exact(&mut body) {
                Ok(()) => {}
                Err(err) if err.kind() == ErrorKind::UnexpectedEof => break,
                Err(_) => return Err(DbError::ReadError),
            }

            let entry_type = body[0];
            let checksum_offset = body.len() - 4;
            let payload = &body[1..checksum_offset];
            let expected_checksum = u32::from_le_bytes(body[checksum_offset..].try_into().unwrap());
            let actual_checksum = crc32_for_entry(entry_type, payload);

            if actual_checksum != expected_checksum {
                break;
            }

            entries.push(WalEntry::decode(entry_type, payload)?);
        }

        Ok(entries)
    }

    pub fn sync(&mut self) -> Result<(), DbError> {
        self.flush()?;
        self.writer.get_ref().sync_all().map_err(DbError::Io)
    }

    pub fn truncate(&mut self) -> Result<(), DbError> {
        self.flush()?;
        self.writer
            .get_mut()
            .set_len(0)
            .map_err(|_| DbError::WriteError)?;
        self.writer
            .seek(SeekFrom::Start(0))
            .map_err(|_| DbError::SeekError)?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), DbError> {
        self.writer.flush().map_err(|_| DbError::WriteError)
    }
}
