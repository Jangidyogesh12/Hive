// Tests PropertyRecord defaults and byte serialization roundtrip.
use crate::store::property_record::PropertyRecord;
use crate::types::NIL_ID;

#[test]
// Verifies constructor initializes defaults and NIL links.
fn new_sets_default_fields() {
    let record = PropertyRecord::new(42);

    assert_eq!(record.id, 42);
    assert_eq!(record.key_hash, NIL_ID);
    assert_eq!(record.value_type, 0);
    assert_eq!(record.value_inline, [0u8; 15]);
    assert_eq!(record.next_property, NIL_ID);
    assert_eq!(record.flags, 0);
    assert_eq!(record.reserved, 0);
}

#[test]
// Verifies PropertyRecord serialization roundtrip is lossless.
fn to_bytes_and_from_bytes_roundtrip() {
    let mut record = PropertyRecord::new(7);
    record.key_hash = 99;
    record.value_type = 4;
    record.value_inline = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    record.next_property = 123;
    record.flags = 0xAABB_CCDD;
    record.reserved = 0x0102_0304;

    let bytes = record.to_bytes();
    let got = PropertyRecord::from_bytes(bytes);

    assert_eq!(got, record);   
}
