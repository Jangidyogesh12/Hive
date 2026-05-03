// Tests EdgeStore append/read behavior, ordering, and out-of-bounds reads.
use super::super::utils::utils::{cleanup_file, temp_file};
use crate::errors::DbError;
use crate::store::edge::record::EdgeRecord;
use crate::store::edge::store::EdgeStore;

#[test]
// Verifies an appended edge can be read back unchanged.
fn append_then_read_returns_same_record() {
    let path = temp_file("edge_append_then_read_returns_same_record");
    let mut store = EdgeStore::open(&path).unwrap();

    let mut record = EdgeRecord::new(1);
    record.src = 11;
    record.dst = 22;
    record.next_out_edge = 33;
    record.next_in_edge = 44;
    record.first_property = 55;
    record.edge_type = 66;
    record.flags = 7;

    store.append(record).unwrap();

    let got = store.read(0).unwrap();
    assert_eq!(got, record);

    cleanup_file(&path);
}

#[test]
// Verifies append preserves insertion order for sequential reads.
fn append_multiple_records_keeps_order() {
    let path = temp_file("edge_append_multiple_records_keeps_order");
    let mut store = EdgeStore::open(&path).unwrap();

    let mut r1 = EdgeRecord::new(1);
    r1.flags = 11;

    let mut r2 = EdgeRecord::new(2);
    r2.flags = 22;

    store.append(r1).unwrap();
    store.append(r2).unwrap();

    assert_eq!(store.read(0).unwrap(), r1);
    assert_eq!(store.read(1).unwrap(), r2);

    cleanup_file(&path);
}

#[test]
// Verifies reading past the last record returns ReadError.
fn read_out_of_bounds_returns_read_error() {
    let path = temp_file("edge_read_out_of_bounds_returns_read_error");
    let mut store = EdgeStore::open(&path).unwrap();

    let mut record = EdgeRecord::new(1);
    record.flags = 3;
    store.append(record).unwrap();

    let result = store.read(1);
    assert!(matches!(result, Err(DbError::ReadError)));

    cleanup_file(&path);
}
