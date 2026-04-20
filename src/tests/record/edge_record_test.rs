// Tests EdgeRecord defaults and byte serialization roundtrip.
use crate::store::edge_record::EdgeRecord;
use crate::types::NIL_ID;

#[test]
// Verifies constructor initializes defaults and NIL links.
fn new_sets_default_fields() {
    let record = EdgeRecord::new(42);

    assert_eq!(record.id, 42);
    assert_eq!(record.src, NIL_ID);
    assert_eq!(record.dst, NIL_ID);
    assert_eq!(record.next_out_edge, NIL_ID);
    assert_eq!(record.next_in_edge, NIL_ID);
    assert_eq!(record.first_property, NIL_ID);
    assert_eq!(record.edge_type, 0);
    assert_eq!(record.flags, 0);
}

#[test]
// Verifies EdgeRecord serialization roundtrip is lossless.
fn to_bytes_and_from_bytes_roundtrip() {
    let mut record = EdgeRecord::new(9);
    record.src = 10;
    record.dst = 20;
    record.next_out_edge = 30;
    record.next_in_edge = 40;
    record.first_property = 50;
    record.edge_type = 60;
    record.flags = 70;

    let bytes = record.to_bytes();
    let got = EdgeRecord::from_bytes(bytes);

    assert_eq!(got, record);
}
