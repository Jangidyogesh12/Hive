// Tests NodeStore update behavior and record isolation guarantees.
use super::super::utils::utils::{cleanup_file, temp_file};
use crate::store::node_record::NodeRecord;
use crate::store::node_store::NodeStore;

#[test]
// Verifies update replaces data at the targeted record index.
fn update_overwrites_existing_record() {
    let path = temp_file("update_overwrites_existing_record");
    let mut store = NodeStore::open(&path).unwrap();

    let mut original = NodeRecord::new(1);
    original.flags = 10;
    store.append(original).unwrap();

    let mut updated = NodeRecord::new(1);
    updated.first_out_edge = 55;
    updated.first_in_edge = 77;
    updated.first_property = 99;
    updated.flags = 42;
    updated.label_id = 7;

    store.update(0, updated).unwrap();

    let got = store.read(0).unwrap();
    assert_eq!(got, updated);

    cleanup_file(&path);
}

#[test]
// Verifies updating one record does not alter neighboring records.
fn update_keeps_other_records_unchanged() {
    let path = temp_file("update_keeps_other_records_unchanged");
    let mut store = NodeStore::open(&path).unwrap();

    let mut first = NodeRecord::new(1);
    first.flags = 11;

    let mut second = NodeRecord::new(2);
    second.flags = 22;

    store.append(first).unwrap();
    store.append(second).unwrap();

    let mut second_updated = second;
    second_updated.first_property = 1234;
    second_updated.flags = 99;

    store.update(1, second_updated).unwrap();

    assert_eq!(store.read(0).unwrap(), first);
    assert_eq!(store.read(1).unwrap(), second_updated);

    cleanup_file(&path);
}
