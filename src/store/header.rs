#[repr(C)]
#[derive(Debug, Clone, Copy)]

pub struct DbHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub node_count: u64,
}
