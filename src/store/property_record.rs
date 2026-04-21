//! Property record layout and byte conversion helpers.
use crate::types::NIL_ID;
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]

// Fixed-size on-disk representation of a property.
pub struct PropertyRecord {
    pub id: u64,                // Logical property identifier.
    pub key_hash: u64,          // Hash of the property key name.
    pub value_type: u8,         // Encoded value kind (int, float, bool, string, etc.).
    pub value_inline: [u8; 15], // Inline bytes for small values or external value pointer payload.
    pub next_property: u64,     // Link to next property record in the chain, or NIL_ID.
    pub flags: u32,             // Bitflags for property state.
    pub reserved: u32,          // Reserved bytes for future fields.
}

// Serialized PropertyRecord bytes.
pub type PropertyRecordBytes = [u8; PropertyRecord::SIZE];

impl PropertyRecord {
    // Number of bytes occupied by one serialized property record.
    pub const SIZE: usize = 48;

    // Creates a new property record with NIL links and zeroed flags.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            key_hash: NIL_ID,
            value_type: 0,
            value_inline: [0; 15],
            next_property: NIL_ID,
            flags: 0,
            reserved: 0,
        }
    }

    // Serializes a property record into its fixed-size little-endian format.
    pub fn to_bytes(self) -> PropertyRecordBytes {
        let mut buf = [0u8; Self::SIZE];
        buf[0..8].copy_from_slice(&self.id.to_le_bytes());
        buf[8..16].copy_from_slice(&self.key_hash.to_le_bytes());
        buf[16..17].copy_from_slice(&self.value_type.to_le_bytes());
        buf[17..32].copy_from_slice(&self.value_inline);
        buf[32..40].copy_from_slice(&self.next_property.to_le_bytes());
        buf[40..44].copy_from_slice(&self.flags.to_le_bytes());
        buf[44..48].copy_from_slice(&self.reserved.to_le_bytes());

        return buf;
    }

    // Deserializes a property record from its fixed-size byte representation.
    pub fn from_bytes(buf: PropertyRecordBytes) -> Self {
        return Self {
            id: u64::from_le_bytes(buf[0..8].try_into().unwrap()),
            key_hash: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
            value_type: u8::from_le_bytes(buf[16..17].try_into().unwrap()),
            value_inline: buf[17..32].try_into().unwrap(),
            next_property: u64::from_le_bytes(buf[32..40].try_into().unwrap()),
            flags: u32::from_le_bytes(buf[40..44].try_into().unwrap()),
            reserved: u32::from_le_bytes(buf[44..48].try_into().unwrap()),
        };
    }
}
