pub const NULL: u8 = 0;
pub const INTEGER: u8 = 1;
pub const FLOAT: u8 = 2;
pub const BOOLEAN: u8 = 3;
pub const STRING: u8 = 4;
pub const LONG_STRING: u8 = 5;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Integer(i64),
    Float(f64),
    Boolean(bool),
    String(String),
}

impl Value {
    /// Encodes the value into a type tag and a 15-byte inline buffer.
    /// Long strings set `LONG_STRING` type and store the offset separately.
    pub fn to_inline_bytes(&self) -> (u8, [u8; 15]) {
        match self {
            Value::Null => (NULL, [0u8; 15]),
            Value::Integer(n) => {
                let mut buf = [0u8; 15];
                buf[..8].copy_from_slice(&n.to_le_bytes());
                (INTEGER, buf)
            }
            Value::Float(f) => {
                let mut buf = [0u8; 15];
                buf[..8].copy_from_slice(&f.to_le_bytes());
                (FLOAT, buf)
            }
            Value::Boolean(b) => {
                let mut buf = [0u8; 15];
                buf[0] = *b as u8;
                (BOOLEAN, buf)
            }
            Value::String(s) => {
                let bytes = s.as_bytes();
                if bytes.len() <= 15 {
                    let mut buf = [0u8; 15];
                    buf[..bytes.len()].copy_from_slice(bytes);
                    (STRING, buf)
                } else {
                    (LONG_STRING, [0u8; 15])
                }
            }
        }
    }

    /// Decodes a value from a type tag and a 15-byte inline buffer.
    pub fn from_bytes(value_type: u8, value_inline: [u8; 15]) -> Self {
        match value_type {
            NULL => Value::Null,
            INTEGER => {
                let n = i64::from_le_bytes(value_inline[..8].try_into().unwrap());
                Value::Integer(n)
            }
            FLOAT => {
                let f = f64::from_le_bytes(value_inline[..8].try_into().unwrap());
                Value::Float(f)
            }
            BOOLEAN => Value::Boolean(value_inline[0] != 0),
            STRING => {
                let end = value_inline.iter().position(|&b| b == 0).unwrap_or(15);
                Value::String(String::from_utf8_lossy(&value_inline[..end]).into_owned())
            }
            _ => Value::Null,
        }
    }
}

/// Computes the FNV-1a 64-bit hash of the given key string, used for
/// property key lookups.
pub fn hash_key(key: &str) -> u64 {
    const FNV_OFFSET: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;

    let mut hash = FNV_OFFSET;

    for byte in key.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}
