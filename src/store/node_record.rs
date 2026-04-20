//! Node record layout and byte conversion helpers.
use crate::types::NIL_ID;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Fixed-size on-disk representation of a node.
pub struct NodeRecord {
    pub id: u64,             // Logical node identifier.
    pub first_out_edge: u64, // Link to first outgoing edge, or NIL_ID.
    pub first_in_edge: u64,  // Link to first incoming edge, or NIL_ID.
    pub first_property: u64, // Link to first property, or NIL_ID.
    pub flags: u32,          // Bitflags for node state.
    pub reserved: u32,       // Reserved bytes for future fields.
}

// Serialized NodeRecord bytes.
pub type NodeRecordBytes = [u8; NodeRecord::SIZE];

impl NodeRecord {
    // Number of bytes occupied by one serialized node record.
    pub const SIZE: usize = 40;

    // Creates a new node record with NIL links and zeroed flags.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            first_out_edge: NIL_ID,
            first_in_edge: NIL_ID,
            first_property: NIL_ID,
            flags: 0,
            reserved: 0,
        }
    }

    // Serializes a node record into its fixed-size little-endian format.
    pub fn to_bytes(self) -> NodeRecordBytes {
        let mut buf = [0u8; Self::SIZE];
        buf[0..8].copy_from_slice(&self.id.to_le_bytes());
        buf[8..16].copy_from_slice(&self.first_out_edge.to_le_bytes());
        buf[16..24].copy_from_slice(&self.first_in_edge.to_le_bytes());
        buf[24..32].copy_from_slice(&self.first_property.to_le_bytes());
        buf[32..36].copy_from_slice(&self.flags.to_le_bytes());
        buf[36..40].copy_from_slice(&self.reserved.to_le_bytes());

        buf
    }

    // Deserializes a node record from its fixed-size byte representation.
    pub fn from_bytes(buf: NodeRecordBytes) -> Self {
        Self {
            id: u64::from_le_bytes(buf[0..8].try_into().unwrap()),
            first_out_edge: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
            first_in_edge: u64::from_le_bytes(buf[16..24].try_into().unwrap()),
            first_property: u64::from_le_bytes(buf[24..32].try_into().unwrap()),
            flags: u32::from_le_bytes(buf[32..36].try_into().unwrap()),
            reserved: u32::from_le_bytes(buf[36..40].try_into().unwrap()),
        }
    }
}
