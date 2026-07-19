use crate::errors::DbError;
use crate::storage::page::format::PAGE_SIZE;
use crate::storage::pager::Lsn;
use crate::value::Value;

pub type TxId = u64;

#[derive(Debug, Clone, PartialEq)]
pub struct WalProperty {
    pub key: String,
    pub value: Value,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalEntryType {
    Begin = 1,
    PageImage = 2,
    Commit = 3,
    Checkpoint = 4,
}

impl WalEntryType {
    pub fn from_byte(byte: u8) -> Result<Self, DbError> {
        match byte {
            1 => Ok(Self::Begin),
            2 => Ok(Self::PageImage),
            3 => Ok(Self::Commit),
            4 => Ok(Self::Checkpoint),
            _ => Err(DbError::ReadError),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WalEntry {
    Begin {
        tx_id: TxId,
        lsn: Lsn,
    },
    PageImage {
        tx_id: TxId,
        lsn: Lsn,
        page_id: u32,
        page_lsn: Lsn,
        bytes: Box<[u8; PAGE_SIZE]>,
    },
    Commit {
        tx_id: TxId,
        lsn: Lsn,
    },
    Checkpoint {
        lsn: Lsn,
    },
}

impl WalEntry {
    pub fn entry_type(&self) -> WalEntryType {
        match self {
            Self::Begin { .. } => WalEntryType::Begin,
            Self::PageImage { .. } => WalEntryType::PageImage,
            Self::Commit { .. } => WalEntryType::Commit,
            Self::Checkpoint { .. } => WalEntryType::Checkpoint,
        }
    }

    pub fn lsn(&self) -> Lsn {
        match self {
            Self::Begin { lsn, .. }
            | Self::PageImage { lsn, .. }
            | Self::Commit { lsn, .. }
            | Self::Checkpoint { lsn, .. } => *lsn,
        }
    }

    pub fn tx_id(&self) -> Option<TxId> {
        match self {
            Self::Begin { tx_id, .. }
            | Self::PageImage { tx_id, .. }
            | Self::Commit { tx_id, .. } => Some(*tx_id),
            Self::Checkpoint { .. } => None,
        }
    }

    pub fn encode_payload(&self) -> Result<Vec<u8>, DbError> {
        let mut buf = Vec::new();
        match self {
            Self::Begin { tx_id, lsn } => {
                buf.extend_from_slice(&tx_id.to_le_bytes());
                buf.extend_from_slice(&lsn.to_le_bytes());
            }
            Self::PageImage {
                tx_id,
                lsn,
                page_id,
                page_lsn,
                bytes,
            } => {
                buf.extend_from_slice(&tx_id.to_le_bytes());
                buf.extend_from_slice(&lsn.to_le_bytes());
                buf.extend_from_slice(&page_id.to_le_bytes());
                buf.extend_from_slice(&page_lsn.to_le_bytes());
                buf.extend_from_slice(bytes.as_ref());
            }
            Self::Commit { tx_id, lsn } => {
                buf.extend_from_slice(&tx_id.to_le_bytes());
                buf.extend_from_slice(&lsn.to_le_bytes());
            }
            Self::Checkpoint { lsn } => {
                buf.extend_from_slice(&lsn.to_le_bytes());
            }
        }
        Ok(buf)
    }

    pub fn decode(entry_type: u8, payload: &[u8]) -> Result<Self, DbError> {
        match WalEntryType::from_byte(entry_type)? {
            WalEntryType::Begin => {
                if payload.len() < 16 {
                    return Err(DbError::ReadError);
                }
                let tx_id = u64::from_le_bytes(payload[0..8].try_into().unwrap());
                let lsn = u64::from_le_bytes(payload[8..16].try_into().unwrap());
                Ok(Self::Begin { tx_id, lsn })
            }
            WalEntryType::PageImage => {
                if payload.len() < 24 + PAGE_SIZE {
                    return Err(DbError::ReadError);
                }
                let tx_id = u64::from_le_bytes(payload[0..8].try_into().unwrap());
                let lsn = u64::from_le_bytes(payload[8..16].try_into().unwrap());
                let page_id = u32::from_le_bytes(payload[16..20].try_into().unwrap());
                let page_lsn = u64::from_le_bytes(payload[20..28].try_into().unwrap());
                let mut bytes = Box::new([0u8; PAGE_SIZE]);
                bytes.copy_from_slice(&payload[28..28 + PAGE_SIZE]);
                Ok(Self::PageImage {
                    tx_id,
                    lsn,
                    page_id,
                    page_lsn,
                    bytes,
                })
            }
            WalEntryType::Commit => {
                if payload.len() < 16 {
                    return Err(DbError::ReadError);
                }
                let tx_id = u64::from_le_bytes(payload[0..8].try_into().unwrap());
                let lsn = u64::from_le_bytes(payload[8..16].try_into().unwrap());
                Ok(Self::Commit { tx_id, lsn })
            }
            WalEntryType::Checkpoint => {
                if payload.len() < 8 {
                    return Err(DbError::ReadError);
                }
                let lsn = u64::from_le_bytes(payload[0..8].try_into().unwrap());
                Ok(Self::Checkpoint { lsn })
            }
        }
    }
}
