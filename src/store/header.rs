// On-disk database header layout.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
// Fixed-size metadata stored at the start of a database file.
pub struct DbHeader {
    pub magic: [u8; 8],  // File magic bytes used for format identification.
    pub version: u32,    // Format version for compatibility checks.
    pub node_count: u64, // Total number of node records persisted.
}
