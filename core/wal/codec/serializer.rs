use crate::errors::DbError;
use std::io::Write;

use crate::wal::wal_entry::WalEntry;

pub struct Serializer<'a> {
    writer: &'a mut Vec<u8>,
}

impl<'a> Serializer<'a> {
    pub fn new(writer: &'a mut Vec<u8>) -> Self {
        Self { writer }
    }

    pub fn write_bytes(&mut self, value: &[u8]) -> Result<(), DbError> {
        self.writer
            .write_all(value)
            .map_err(|_| DbError::WriteError)
    }

    pub fn write_u8(&mut self, value: u8) -> Result<(), DbError> {
        self.writer
            .write_all(&[value])
            .map_err(|_| DbError::WriteError)
    }

    pub fn write_u32(&mut self, value: u32) -> Result<(), DbError> {
        self.writer
            .write_all(&value.to_le_bytes())
            .map_err(|_| DbError::WriteError)
    }

    pub fn write_u64(&mut self, value: u64) -> Result<(), DbError> {
        self.writer
            .write_all(&value.to_le_bytes())
            .map_err(|_| DbError::WriteError)
    }

    pub fn write_entries(&mut self, entries: &[WalEntry]) -> Result<(), DbError> {
        self.write_u32(entries.len() as u32)?;
        for entry in entries {
            let entry_type = entry.entry_type() as u8;
            let payload = entry.encode_payload()?;
            self.write_u8(entry_type)?;
            self.write_u32(payload.len() as u32)?;
            self.writer
                .write_all(&payload)
                .map_err(|_| DbError::WriteError)?;
        }
        Ok(())
    }
}
