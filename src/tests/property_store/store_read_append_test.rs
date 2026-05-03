// Tests PropertyStore append/read behavior, ordering, and out-of-bounds reads.
use super::super::utils::utils::{cleanup_file, temp_file};
use crate::errors::DbError;
use crate::store::property::record::PropertyRecord;
use crate::store::property::store::PropertyStore;

#[test]
// Verifies an appended property can be read back unchanged.
fn append_then_read_returns_same_record() {
    let path = temp_file("property_append_then_read_returns_same_record");
    let mut store = PropertyStore::open(&path).unwrap();

    let mut record = PropertyRecord::new(1);
    record.key_hash = 100;
    record.value_type = 4;
    record.value_inline = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    record.next_property = 200;
    record.flags = 7;
    record.reserved = 9;

    store.append(record).unwrap();

    let got = store.read(0).unwrap();
    assert_eq!(got, record);

    cleanup_file(&path);
}

#[test]
// Verifies append preserves insertion order for sequential reads.
fn append_multiple_records_keeps_order() {
    let path = temp_file("property_append_multiple_records_keeps_order");
    let mut store = PropertyStore::open(&path).unwrap();

    let mut r1 = PropertyRecord::new(1);
    r1.flags = 11;

    let mut r2 = PropertyRecord::new(2);
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
    let path = temp_file("property_read_out_of_bounds_returns_read_error");
    let mut store = PropertyStore::open(&path).unwrap();

    let mut record = PropertyRecord::new(1);
    record.flags = 3;
    store.append(record).unwrap();

    let result = store.read(1);
    assert!(matches!(result, Err(DbError::ReadError)));

    cleanup_file(&path);
}
