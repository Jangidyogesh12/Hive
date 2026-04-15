use crate::types::NIL_ID;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]

pub struct EdgeRecord {
    pub id: u64,
    pub src: u64,
    pub dst: u64,
    pub next_out_edge: u64,
    pub next_in_edge: u64,
    pub first_property: u64,
    pub edge_type: u32,
    pub flags: u32,
}

pub type EdgeRecordBytes = [u8; EdgeRecord::SIZE];

impl EdgeRecord {
    pub const SIZE: usize = 56;
    pub fn new(id: u64) -> Self {
        Self {
            id,
            src: NIL_ID,
            dst: NIL_ID,
            next_out_edge: NIL_ID,
            next_in_edge: NIL_ID,
            first_property: NIL_ID,
            edge_type: 0,
            flags: 0,
        }
    }

    pub fn to_bytes(self) -> EdgeRecordBytes {
        let mut buf = [0u8; Self::SIZE];
        buf[0..8].copy_from_slice(&self.id.to_le_bytes());
        buf[8..16].copy_from_slice(&self.src.to_le_bytes());
        buf[16..24].copy_from_slice(&self.dst.to_le_bytes());
        buf[24..32].copy_from_slice(&self.next_out_edge.to_le_bytes());
        buf[32..40].copy_from_slice(&self.next_in_edge.to_le_bytes());
        buf[40..48].copy_from_slice(&self.next_in_edge.to_le_bytes());
        buf[48..52].copy_from_slice(&self.edge_type.to_le_bytes());
        buf[52..56].copy_from_slice(&self.flags.to_le_bytes());

        return buf;
    }

    pub fn from_bytes(buf: EdgeRecordBytes) -> Self {
        return Self {
            id: u64::from_le_bytes(buf[0..8].try_into().unwrap()),
            src: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
            dst: u64::from_le_bytes(buf[16..24].try_into().unwrap()),
            next_out_edge: u64::from_le_bytes(buf[24..32].try_into().unwrap()),
            next_in_edge: u64::from_le_bytes(buf[32..40].try_into().unwrap()),
            first_property: u64::from_le_bytes(buf[40..48].try_into().unwrap()),
            edge_type: u32::from_le_bytes(buf[48..52].try_into().unwrap()),
            flags: u32::from_le_bytes(buf[52..56].try_into().unwrap()),
        };
    }
}
