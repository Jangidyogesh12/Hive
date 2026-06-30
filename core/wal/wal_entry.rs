use crate::errors::DbError;
use crate::types::{EdgeId, NodeId};
use crate::value::Value;
use std::io::Cursor;

use super::utils::*;

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
    pub(super) fn from_byte(byte: u8) -> Result<Self, DbError> {
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
    pub(super) fn entry_type(&self) -> WalEntryType {
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

    pub(super) fn encode_payload(&self) -> Result<Vec<u8>, DbError> {
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

    pub(super) fn decode(entry_type: u8, payload: &[u8]) -> Result<Self, DbError> {
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
