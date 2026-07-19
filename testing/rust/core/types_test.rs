use crate::types::{NIL_ID, is_nil_id, pack_record_id, unpack_record_id};

#[test]
fn pack_zero_ids() {
    let id = pack_record_id(0, 0);
    assert_eq!(id, 0);
    assert_eq!(unpack_record_id(id), (0, 0));
}

#[test]
fn pack_page_only() {
    let id = pack_record_id(5, 0);
    assert_eq!(unpack_record_id(id), (5, 0));
}

#[test]
fn pack_slot_only() {
    let id = pack_record_id(0, 3);
    assert_eq!(unpack_record_id(id), (0, 3));
}

#[test]
fn pack_both() {
    let id = pack_record_id(42, 7);
    assert_eq!(unpack_record_id(id), (42, 7));
}

#[test]
fn pack_max_values() {
    let id = pack_record_id(u32::MAX, u16::MAX);
    assert_eq!(unpack_record_id(id), (u32::MAX, u16::MAX));
}

#[test]
fn pack_large_page_id() {
    let id = pack_record_id(1_000_000, 100);
    assert_eq!(unpack_record_id(id), (1_000_000, 100));
}

#[test]
fn is_nil_id_works() {
    assert!(is_nil_id(NIL_ID));
    assert!(!is_nil_id(0));
    assert!(!is_nil_id(1));
}
