use super::codec::{Deserializer, Serializer};
use crate::errors::DbError;
use crate::types::{EdgeId, NodeId};
use crate::value::Value;

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
        let mut ser = Serializer::new(&mut buf);

        match self {
            Self::CreateNode {
                node_id,
                label,
                properties,
            } => {
                ser.write_u64(*node_id)?;
                ser.write_string(label)?;
                ser.write_properties(properties)?;
            }
            Self::CreateEdge {
                edge_id,
                src,
                dst,
                label,
                properties,
            } => {
                ser.write_u64(*edge_id)?;
                ser.write_u64(*src)?;
                ser.write_u64(*dst)?;
                ser.write_string(label)?;
                ser.write_properties(properties)?;
            }
            Self::UpdateNode {
                node_id,
                key,
                value,
            } => {
                ser.write_u64(*node_id)?;
                ser.write_string(key)?;
                ser.write_value(value)?;
            }
            Self::UpdateEdge {
                edge_id,
                key,
                value,
            } => {
                ser.write_u64(*edge_id)?;
                ser.write_string(key)?;
                ser.write_value(value)?;
            }
            Self::DeleteNode { node_id } => ser.write_u64(*node_id)?,
            Self::DeleteEdge { edge_id } => ser.write_u64(*edge_id)?,
            Self::Checkpoint => {}
            Self::Transaction { entries } => ser.write_entries(entries)?,
        }

        Ok(buf)
    }

    pub(super) fn decode(entry_type: u8, payload: &[u8]) -> Result<Self, DbError> {
        let mut de = Deserializer::new(payload);

        match WalEntryType::from_byte(entry_type)? {
            WalEntryType::CreateNode => Ok(Self::CreateNode {
                node_id: de.read_u64()?,
                label: de.read_string()?,
                properties: de.read_properties()?,
            }),
            WalEntryType::CreateEdge => Ok(Self::CreateEdge {
                edge_id: de.read_u64()?,
                src: de.read_u64()?,
                dst: de.read_u64()?,
                label: de.read_string()?,
                properties: de.read_properties()?,
            }),
            WalEntryType::UpdateNode => Ok(Self::UpdateNode {
                node_id: de.read_u64()?,
                key: de.read_string()?,
                value: de.read_value()?,
            }),
            WalEntryType::UpdateEdge => Ok(Self::UpdateEdge {
                edge_id: de.read_u64()?,
                key: de.read_string()?,
                value: de.read_value()?,
            }),
            WalEntryType::DeleteNode => Ok(Self::DeleteNode {
                node_id: de.read_u64()?,
            }),
            WalEntryType::DeleteEdge => Ok(Self::DeleteEdge {
                edge_id: de.read_u64()?,
            }),
            WalEntryType::Checkpoint => Ok(Self::Checkpoint),
            WalEntryType::Transaction => Ok(Self::Transaction {
                entries: de.read_entries()?,
            }),
        }
    }
}
