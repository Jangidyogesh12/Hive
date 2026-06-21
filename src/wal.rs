use crate::errors::DbError;
use crate::types::{EdgeId, NodeId};
use crate::value::{BOOLEAN, FLOAT, INTEGER, LONG_STRING, NULL, STRING, Value};
use crc32fast::Hasher;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Cursor, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::Path;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalEntryType {
    CreateNode = 1,
    CreateEdge = 2,
    UpdateNode = 3,
    UpdateEdge = 4,
    DeleteNode = 5,
    DeleteEdge = 6,
    Checkpoint = 7,
    Transaction = 8,
}

impl WalEntryType {
    fn from_byte(byte: u8) -> Result<Self, DbError> {
        match byte {
            1 => Ok(Self::CreateNode),
            2 => Ok(Self::CreateEdge),
            3 => Ok(Self::UpdateNode),
            4 => Ok(Self::UpdateEdge),
            5 => Ok(Self::DeleteNode),
            6 => Ok(Self::DeleteEdge),
            7 => Ok(Self::Checkpoint),
            8 => Ok(Self::Transaction),
            _ => Err(DbError::ReadError),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WalProperty {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WalEntry {
    CreateNode {
        node_id: NodeId,
        label: String,
        properties: Vec<WalProperty>,
    },
    CreateEdge {
        edge_id: EdgeId,
        src: NodeId,
        dst: NodeId,
        label: String,
        properties: Vec<WalProperty>,
    },
    UpdateNode {
        node_id: NodeId,
        key: String,
        value: Value,
    },
    UpdateEdge {
        edge_id: EdgeId,
        key: String,
        value: Value,
    },
    DeleteNode {
        node_id: NodeId,
    },
    DeleteEdge {
        edge_id: EdgeId,
    },
    Checkpoint,
    Transaction {
        entries: Vec<WalEntry>,
    },
}

impl WalEntry {
    fn entry_type(&self) -> WalEntryType {
        match self {
            Self::CreateNode { .. } => WalEntryType::CreateNode,
            Self::CreateEdge { .. } => WalEntryType::CreateEdge,
            Self::UpdateNode { .. } => WalEntryType::UpdateNode,
            Self::UpdateEdge { .. } => WalEntryType::UpdateEdge,
            Self::DeleteNode { .. } => WalEntryType::DeleteNode,
            Self::DeleteEdge { .. } => WalEntryType::DeleteEdge,
            Self::Checkpoint => WalEntryType::Checkpoint,
            Self::Transaction { .. } => WalEntryType::Transaction,
        }
    }

    fn encode_payload(&self) -> Result<Vec<u8>, DbError> {
        let mut buf = Vec::new();

        match self {
            Self::CreateNode {
                node_id,
                label,
                properties,
            } => {
                write_u64(&mut buf, *node_id)?;
                write_string(&mut buf, label)?;
                write_properties(&mut buf, properties)?;
            }
            Self::CreateEdge {
                edge_id,
                src,
                dst,
                label,
                properties,
            } => {
                write_u64(&mut buf, *edge_id)?;
                write_u64(&mut buf, *src)?;
                write_u64(&mut buf, *dst)?;
                write_string(&mut buf, label)?;
                write_properties(&mut buf, properties)?;
            }
            Self::UpdateNode {
                node_id,
                key,
                value,
            } => {
                write_u64(&mut buf, *node_id)?;
                write_string(&mut buf, key)?;
                write_value(&mut buf, value)?;
            }
            Self::UpdateEdge {
                edge_id,
                key,
                value,
            } => {
                write_u64(&mut buf, *edge_id)?;
                write_string(&mut buf, key)?;
                write_value(&mut buf, value)?;
            }
            Self::DeleteNode { node_id } => write_u64(&mut buf, *node_id)?,
            Self::DeleteEdge { edge_id } => write_u64(&mut buf, *edge_id)?,
            Self::Checkpoint => {}
            Self::Transaction { entries } => write_entries(&mut buf, entries)?,
        }

        Ok(buf)
    }

    fn decode(entry_type: u8, payload: &[u8]) -> Result<Self, DbError> {
        let mut cursor = Cursor::new(payload);

        match WalEntryType::from_byte(entry_type)? {
            WalEntryType::CreateNode => Ok(Self::CreateNode {
                node_id: read_u64(&mut cursor)?,
                label: read_string(&mut cursor)?,
                properties: read_properties(&mut cursor)?,
            }),
            WalEntryType::CreateEdge => Ok(Self::CreateEdge {
                edge_id: read_u64(&mut cursor)?,
                src: read_u64(&mut cursor)?,
                dst: read_u64(&mut cursor)?,
                label: read_string(&mut cursor)?,
                properties: read_properties(&mut cursor)?,
            }),
            WalEntryType::UpdateNode => Ok(Self::UpdateNode {
                node_id: read_u64(&mut cursor)?,
                key: read_string(&mut cursor)?,
                value: read_value(&mut cursor)?,
            }),
            WalEntryType::UpdateEdge => Ok(Self::UpdateEdge {
                edge_id: read_u64(&mut cursor)?,
                key: read_string(&mut cursor)?,
                value: read_value(&mut cursor)?,
            }),
            WalEntryType::DeleteNode => Ok(Self::DeleteNode {
                node_id: read_u64(&mut cursor)?,
            }),
            WalEntryType::DeleteEdge => Ok(Self::DeleteEdge {
                edge_id: read_u64(&mut cursor)?,
            }),
            WalEntryType::Checkpoint => Ok(Self::Checkpoint),
            WalEntryType::Transaction => Ok(Self::Transaction {
                entries: read_entries(&mut cursor)?,
            }),
        }
    }
}

pub struct Wal {
    reader: File,
    writer: BufWriter<File>,
}

impl Wal {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let reader = OpenOptions::new()
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
            let expected_checksum =
                u32::from_le_bytes(body[checksum_offset..].try_into().unwrap());
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

fn write_u8(buf: &mut Vec<u8>, value: u8) -> Result<(), DbError> {
    buf.write_all(&[value]).map_err(|_| DbError::WriteError)
}

fn write_u32(buf: &mut Vec<u8>, value: u32) -> Result<(), DbError> {
    buf.write_all(&value.to_le_bytes())
        .map_err(|_| DbError::WriteError)
}

fn write_u64(buf: &mut Vec<u8>, value: u64) -> Result<(), DbError> {
    buf.write_all(&value.to_le_bytes())
        .map_err(|_| DbError::WriteError)
}

fn write_i64(buf: &mut Vec<u8>, value: i64) -> Result<(), DbError> {
    buf.write_all(&value.to_le_bytes())
        .map_err(|_| DbError::WriteError)
}

fn write_f64(buf: &mut Vec<u8>, value: f64) -> Result<(), DbError> {
    buf.write_all(&value.to_le_bytes())
        .map_err(|_| DbError::WriteError)
}

fn write_string(buf: &mut Vec<u8>, value: &str) -> Result<(), DbError> {
    let bytes = value.as_bytes();
    write_u32(buf, bytes.len() as u32)?;
    buf.write_all(bytes).map_err(|_| DbError::WriteError)
}

fn read_u8<R: Read>(reader: &mut R) -> Result<u8, DbError> {
    let mut buf = [0u8; 1];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    Ok(buf[0])
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32, DbError> {
    let mut buf = [0u8; 4];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64, DbError> {
    let mut buf = [0u8; 8];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    Ok(u64::from_le_bytes(buf))
}

fn read_i64<R: Read>(reader: &mut R) -> Result<i64, DbError> {
    let mut buf = [0u8; 8];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    Ok(i64::from_le_bytes(buf))
}

fn read_f64<R: Read>(reader: &mut R) -> Result<f64, DbError> {
    let mut buf = [0u8; 8];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    Ok(f64::from_le_bytes(buf))
}

fn read_string<R: Read>(reader: &mut R) -> Result<String, DbError> {
    let len = read_u32(reader)? as usize;
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .map_err(|_| DbError::ReadError)?;
    String::from_utf8(buf).map_err(|_| DbError::ReadError)
}

fn write_value(buf: &mut Vec<u8>, value: &Value) -> Result<(), DbError> {
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

fn read_value<R: Read>(reader: &mut R) -> Result<Value, DbError> {
    match read_u8(reader)? {
        NULL => Ok(Value::Null),
        INTEGER => Ok(Value::Integer(read_i64(reader)?)),
        FLOAT => Ok(Value::Float(read_f64(reader)?)),
        BOOLEAN => Ok(Value::Boolean(read_u8(reader)? != 0)),
        STRING | LONG_STRING => Ok(Value::String(read_string(reader)?)),
        _ => Err(DbError::ReadError),
    }
}

fn write_properties(buf: &mut Vec<u8>, properties: &[WalProperty]) -> Result<(), DbError> {
    write_u32(buf, properties.len() as u32)?;
    for property in properties {
        write_string(buf, &property.key)?;
        write_value(buf, &property.value)?;
    }
    Ok(())
}

fn write_entries(buf: &mut Vec<u8>, entries: &[WalEntry]) -> Result<(), DbError> {
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

fn read_properties<R: Read>(reader: &mut R) -> Result<Vec<WalProperty>, DbError> {
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

fn read_entries<R: Read>(reader: &mut R) -> Result<Vec<WalEntry>, DbError> {
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

fn crc32_for_entry(entry_type: u8, payload: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(&[entry_type]);
    hasher.update(payload);
    hasher.finalize()
}
