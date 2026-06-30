use crate::errors::DbError;
use crate::value::{BOOLEAN, FLOAT, INTEGER, LONG_STRING, NULL, STRING, Value};
use std::io::{Cursor, Read};

use crate::wal::wal_entry::{WalEntry, WalProperty};

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

    pub fn read_i64(&mut self) -> Result<i64, DbError> {
        let mut buf = [0u8; 8];
        self.reader
            .read_exact(&mut buf)
            .map_err(|_| DbError::ReadError)?;
        Ok(i64::from_le_bytes(buf))
    }

    pub fn read_f64(&mut self) -> Result<f64, DbError> {
        let mut buf = [0u8; 8];
        self.reader
            .read_exact(&mut buf)
            .map_err(|_| DbError::ReadError)?;
        Ok(f64::from_le_bytes(buf))
    }

    pub fn read_string(&mut self) -> Result<String, DbError> {
        let len = self.read_u32()? as usize;
        let mut buf = vec![0u8; len];
        self.reader
            .read_exact(&mut buf)
            .map_err(|_| DbError::ReadError)?;
        String::from_utf8(buf).map_err(|_| DbError::ReadError)
    }

    pub fn read_value(&mut self) -> Result<Value, DbError> {
        match self.read_u8()? {
            NULL => Ok(Value::Null),
            INTEGER => Ok(Value::Integer(self.read_i64()?)),
            FLOAT => Ok(Value::Float(self.read_f64()?)),
            BOOLEAN => Ok(Value::Boolean(self.read_u8()? != 0)),
            STRING | LONG_STRING => Ok(Value::String(self.read_string()?)),
            _ => Err(DbError::ReadError),
        }
    }

    pub fn read_properties(&mut self) -> Result<Vec<WalProperty>, DbError> {
        let count = self.read_u32()? as usize;
        let mut properties = Vec::with_capacity(count);

        for _ in 0..count {
            properties.push(WalProperty {
                key: self.read_string()?,
                value: self.read_value()?,
            });
        }

        Ok(properties)
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
