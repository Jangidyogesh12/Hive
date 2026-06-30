use crate::errors::DbError;
use crate::value::{BOOLEAN, FLOAT, INTEGER, NULL, STRING, Value};
use std::io::Write;

use crate::wal::wal_entry::{WalEntry, WalProperty};

pub struct Serializer<'a> {
    writer: &'a mut Vec<u8>,
}

impl<'a> Serializer<'a> {
    pub fn new(writer: &'a mut Vec<u8>) -> Self {
        Self { writer }
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

    pub fn write_i64(&mut self, value: i64) -> Result<(), DbError> {
        self.writer
            .write_all(&value.to_le_bytes())
            .map_err(|_| DbError::WriteError)
    }

    pub fn write_f64(&mut self, value: f64) -> Result<(), DbError> {
        self.writer
            .write_all(&value.to_le_bytes())
            .map_err(|_| DbError::WriteError)
    }

    pub fn write_string(&mut self, value: &str) -> Result<(), DbError> {
        let bytes = value.as_bytes();
        self.write_u32(bytes.len() as u32)?;
        self.writer
            .write_all(bytes)
            .map_err(|_| DbError::WriteError)
    }

    pub fn write_value(&mut self, value: &Value) -> Result<(), DbError> {
        match value {
            Value::Null => self.write_u8(NULL),
            Value::Integer(n) => {
                self.write_u8(INTEGER)?;
                self.write_i64(*n)
            }
            Value::Float(f) => {
                self.write_u8(FLOAT)?;
                self.write_f64(*f)
            }
            Value::Boolean(b) => {
                self.write_u8(BOOLEAN)?;
                self.write_u8(*b as u8)
            }
            Value::String(s) => {
                self.write_u8(STRING)?;
                self.write_string(s)
            }
        }
    }

    pub fn write_properties(&mut self, properties: &[WalProperty]) -> Result<(), DbError> {
        self.write_u32(properties.len() as u32)?;
        for property in properties {
            self.write_string(&property.key)?;
            self.write_value(&property.value)?;
        }
        Ok(())
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
