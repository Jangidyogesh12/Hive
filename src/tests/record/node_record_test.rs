// Tests NodeRecord defaults and byte serialization roundtrip.
use crate::store::node_record::NodeRecord;
use crate::types::NIL_ID;

#[test]
// Verifies constructor initializes defaults and NIL links.
fn new_sets_default_fields() {
    let record = NodeRecord::new(42);

    assert_eq!(record.id, 42);
    assert_eq!(record.first_out_edge, NIL_ID);
    assert_eq!(record.first_in_edge, NIL_ID);
    assert_eq!(record.first_property, NIL_ID);
    assert_eq!(record.flags, 0);
    assert_eq!(record.reserved, 0);
}

#[test]
// Verifies NodeRecord serialization roundtrip is lossless.
fn to_bytes_and_from_bytes_roundtrip() {
    let mut record = NodeRecord::new(9);
    record.first_out_edge = 10;
    record.first_in_edge = 20;
    record.first_property = 30;
    record.flags = 40;
    record.reserved = 50;

    let bytes = record.to_bytes();
    let got = NodeRecord::from_bytes(bytes);

    assert_eq!(got, record);
}
