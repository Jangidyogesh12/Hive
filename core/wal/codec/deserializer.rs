use crate::errors::DbError;
use std::io::{Cursor, Read};

use crate::wal::wal_entry::WalEntry;

pub struct Deserializer<'a> {
    reader: Cursor<&'a [u8]>,
}

impl<'a> Deserializer<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            reader: Cursor::new(data),
        }
    }

    pub fn read_u8(&mut self) -> Result<u8, DbError> {
        let mut buf = [0u8; 1];
        self.reader
            .read_exact(&mut buf)
            .map_err(|_| DbError::ReadError)?;
        Ok(buf[0])
    }

    pub fn read_u32(&mut self) -> Result<u32, DbError> {
        let mut buf = [0u8; 4];
        self.reader
            .read_exact(&mut buf)
            .map_err(|_| DbError::ReadError)?;
        Ok(u32::from_le_bytes(buf))
    }

    pub fn read_u64(&mut self) -> Result<u64, DbError> {
        let mut buf = [0u8; 8];
        self.reader
            .read_exact(&mut buf)
            .map_err(|_| DbError::ReadError)?;
        Ok(u64::from_le_bytes(buf))
    }

    pub fn read_entries(&mut self) -> Result<Vec<WalEntry>, DbError> {
        let count = self.read_u32()? as usize;
        let mut entries = Vec::with_capacity(count);

        for _ in 0..count {
            let entry_type = self.read_u8()?;
            let payload_len = self.read_u32()? as usize;
            let mut payload = vec![0u8; payload_len];
            self.reader
                .read_exact(&mut payload)
                .map_err(|_| DbError::ReadError)?;
            entries.push(WalEntry::decode(entry_type, &payload)?);
        }

        Ok(entries)
    }
}
