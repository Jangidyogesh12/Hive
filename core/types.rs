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

/// Packs a page_id and slot_id into a single u64 record ID.
///
/// Layout: `[page_id: 32 bits][slot_id: 16 bits][flags: 16 bits]`
///
/// The flags field is reserved for future use (generation counters, type tags, etc).
pub fn pack_record_id(page_id: u32, slot_id: u16) -> u64 {
    (page_id as u64) << 32 | (slot_id as u64) << 16
}

/// Unpacks a record ID into its page_id and slot_id components.
pub fn unpack_record_id(id: u64) -> (u32, u16) {
    let page_id = (id >> 32) as u32;
    let slot_id = ((id >> 16) & 0xFFFF) as u16;
    (page_id, slot_id)
}

/// Returns true if the record ID is the nil/sentinel value.
pub fn is_nil_id(id: u64) -> bool {
    id == NIL_ID
}
