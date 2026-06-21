//! Shared primitive IDs and sentinel constants.

/// Stable identifier type for nodes.
pub type NodeId = u64;

/// Stable identifier type for edges.
pub type EdgeId = u64;

/// Stable identifier type for properties.
pub type PropertyId = u64;

/// Sentinel value used for missing linked-list pointers.
pub const NIL_ID: u64 = u64::MAX;

/// Bit flag marking a record as logically deleted.
pub const DELETED: u32 = 1 << 0;
