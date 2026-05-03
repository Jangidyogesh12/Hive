// Tests NodeStore append/read behavior, ordering, and out-of-bounds reads.
use super::super::utils::utils::{cleanup_file, temp_file};
use crate::errors::DbError;
use crate::store::node::record::NodeRecord;
use crate::store::node::store::NodeStore;

#[test]
// Verifies an appended node can be read back unchanged.
fn append_then_read_returns_same_record() {
    let path = temp_file("append_then_read_returns_same_record");
    let mut store = NodeStore::open(&path).unwrap();

    let mut record = NodeRecord::new(1);

    record.first_out_edge = 100;
    record.first_in_edge = 200;
    record.first_property = 300;
    record.flags = 7;
    record.label_id = 9;

    store.append(record).unwrap();

    let got = store.read(0).unwrap();

    assert_eq!(got, record);

    cleanup_file(&path);
}

#[test]
// Verifies append preserves insertion order for sequential reads.
fn append_multiple_records_keeps_order() {
    let path = temp_file("append_multiple_records_keeps_order");

    let mut store = NodeStore::open(&path).unwrap();

    let mut r1 = NodeRecord::new(1);
    r1.flags = 11;

    let mut r2 = NodeRecord::new(2);
    r2.flags = 21;

    store.append(r1).unwrap();
    store.append(r2).unwrap();

    assert_eq!(store.read(0).unwrap(), r1);
    assert_eq!(store.read(1).unwrap(), r2);

    cleanup_file(&path);
}

#[test]
// Verifies reading past the last record returns ReadError.
fn read_out_of_bounds_returns_read_error() {
    let path = temp_file("read_out_of_bounds_returns_read_error");

    let mut store = NodeStore::open(&path).unwrap();

    let mut record = NodeRecord::new(1);
    record.flags = 3;

    store.append(record).unwrap();

    let result = store.read(1);

    assert!(matches!(result, Err(DbError::ReadError)));

    cleanup_file(&path);
}
