use crate::errors::DbError;
use crate::value::{BOOLEAN, FLOAT, INTEGER, LONG_STRING, NULL, STRING, Value};
use crc32fast::Hasher;
use std::io::{Read, Write};

use super::wal_entry::{WalEntry, WalProperty};

pub(super) fn write_u8(buf: &mut Vec<u8>, value: u8) -> Result<(), DbError> {
    buf.write_all(&[value]).map_err(|_| DbError::WriteError)
}

pub(super) fn write_u32(buf: &mut Vec<u8>, value: u32) -> Result<(), DbError> {
    buf.write_all(&value.to_le_bytes())
        .map_err(|_| DbError::WriteError)
}

pub(super) fn write_u64(buf: &mut Vec<u8>, value: u64) -> Result<(), DbError> {
    buf.write_all(&value.to_le_bytes())
        .map_err(|_| DbError::WriteError)
}

pub(super) fn write_i64(buf: &mut Vec<u8>, value: i64) -> Result<(), DbError> {
    buf.write_all(&value.to_le_bytes())
        .map_err(|_| DbError::WriteError)
}

pub(super) fn write_f64(buf: &mut Vec<u8>, value: f64) -> Result<(), DbError> {
    buf.write_all(&value.to_le_bytes())
        .map_err(|_| DbError::WriteError)
}

pub(super) fn write_string(buf: &mut Vec<u8>, value: &str) -> Result<(), DbError> {
    let bytes = value.as_bytes();
    write_u32(buf, bytes.len() as u32)?;
    buf.write_all(bytes).map_err(|_| DbError::WriteError)
}

pub(super) fn read_u8<R: Read>(reader: &mut R) -> Result<u8, DbError> {
    let mut buf = [0u8; 1];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    Ok(buf[0])
}

pub(super) fn read_u32<R: Read>(reader: &mut R) -> Result<u32, DbError> {
    let mut buf = [0u8; 4];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    Ok(u32::from_le_bytes(buf))
}

pub(super) fn read_u64<R: Read>(reader: &mut R) -> Result<u64, DbError> {
    let mut buf = [0u8; 8];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    Ok(u64::from_le_bytes(buf))
}

pub(super) fn read_i64<R: Read>(reader: &mut R) -> Result<i64, DbError> {
    let mut buf = [0u8; 8];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    Ok(i64::from_le_bytes(buf))
}

pub(super) fn read_f64<R: Read>(reader: &mut R) -> Result<f64, DbError> {
    let mut buf = [0u8; 8];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    Ok(f64::from_le_bytes(buf))
}

pub(super) fn read_string<R: Read>(reader: &mut R) -> Result<String, DbError> {
    let len = read_u32(reader)? as usize;
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    String::from_utf8(buf).map_err(|_| DbError::ReadError)
}

pub(super) fn write_value(buf: &mut Vec<u8>, value: &Value) -> Result<(), DbError> {
    match value {
        Value::Null => write_u8(buf, NULL),
        Value::Integer(n) => {
            write_u8(buf, INTEGER)?;
            write_i64(buf, *n)
        }
        Value::Float(f) => {
            write_u8(buf, FLOAT)?;
            write_f64(buf, *f)
        }
        Value::Boolean(b) => {
            write_u8(buf, BOOLEAN)?;
            write_u8(buf, *b as u8)
        }
        Value::String(s) => {
            write_u8(buf, STRING)?;
            write_string(buf, s)
        }
    }
}

pub(super) fn read_value<R: Read>(reader: &mut R) -> Result<Value, DbError> {
    match read_u8(reader)? {
        NULL => Ok(Value::Null),
        INTEGER => Ok(Value::Integer(read_i64(reader)?)),
        FLOAT => Ok(Value::Float(read_f64(reader)?)),
        BOOLEAN => Ok(Value::Boolean(read_u8(reader)? != 0)),
        STRING | LONG_STRING => Ok(Value::String(read_string(reader)?)),
        _ => Err(DbError::ReadError),
    }
}

pub(super) fn write_properties(buf: &mut Vec<u8>, properties: &[WalProperty]) -> Result<(), DbError> {
    write_u32(buf, properties.len() as u32)?;
    for property in properties {
        write_string(buf, &property.key)?;
        write_value(buf, &property.value)?;
    }
    Ok(())
}

pub(super) fn write_entries(buf: &mut Vec<u8>, entries: &[WalEntry]) -> Result<(), DbError> {
    write_u32(buf, entries.len() as u32)?;
    for entry in entries {
        let entry_type = entry.entry_type() as u8;
        let payload = entry.encode_payload()?;
        write_u8(buf, entry_type)?;
        write_u32(buf, payload.len() as u32)?;
        buf.write_all(&payload).map_err(|_| DbError::WriteError)?;
    }
    Ok(())
}

pub(super) fn read_properties<R: Read>(reader: &mut R) -> Result<Vec<WalProperty>, DbError> {
    let count = read_u32(reader)? as usize;
    let mut properties = Vec::with_capacity(count);

    for _ in 0..count {
        properties.push(WalProperty {
            key: read_string(reader)?,
            value: read_value(reader)?,
        });
    }

    Ok(properties)
}

pub(super) fn read_entries<R: Read>(reader: &mut R) -> Result<Vec<WalEntry>, DbError> {
    let count = read_u32(reader)? as usize;
    let mut entries = Vec::with_capacity(count);

    for _ in 0..count {
        let entry_type = read_u8(reader)?;
        let payload_len = read_u32(reader)? as usize;
        let mut payload = vec![0u8; payload_len];
        reader
            .read_exact(&mut payload)
            .map_err(|_| DbError::ReadError)?;
        entries.push(WalEntry::decode(entry_type, &payload)?);
    }

    Ok(entries)
}

pub(super) fn crc32_for_entry(entry_type: u8, payload: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(&[entry_type]);
    hasher.update(payload);
    hasher.finalize()
}
