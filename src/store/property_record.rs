use crate::types::NIL_ID;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]

pub struct PropertyRecord {
    id: u64,
    key_hash: u64,
    value_type: u8,
    value_inline: [u8; 15],
    next_property: u64,
    flags: u32,
    reserved: u32,
}
