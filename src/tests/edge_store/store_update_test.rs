//! Tests EdgeStore update behavior and record isolation guarantees.
use super::super::utils::utils::{cleanup_file, temp_file};
use crate::store::edge_record::EdgeRecord;
use crate::store::edge_store::EdgeStore;

#[test]
// Verifies update replaces data at the targeted record index.
fn update_overwrites_existing_record() {
    let path = temp_file("edge_update_overwrites_existing_record");
    let mut store = EdgeStore::open(&path).unwrap();

    let mut original = EdgeRecord::new(1);
    original.flags = 1;
    store.append(original).unwrap();

    let mut updated = EdgeRecord::new(1);
    updated.src = 10;
    updated.dst = 20;
    updated.next_out_edge = 30;
    updated.next_in_edge = 40;
    updated.first_property = 50;
    updated.edge_type = 60;
    updated.flags = 99;

    store.update(0, updated).unwrap();

    let got = store.read(0).unwrap();
    assert_eq!(got, updated);

    cleanup_file(&path);
}

#[test]
// Verifies updating one record does not alter neighboring records.
fn update_keeps_other_records_unchanged() {
    let path = temp_file("edge_update_keeps_other_records_unchanged");
    let mut store = EdgeStore::open(&path).unwrap();

    let mut first = EdgeRecord::new(1);
    first.flags = 11;

    let mut second = EdgeRecord::new(2);
    second.flags = 22;

    store.append(first).unwrap();
    store.append(second).unwrap();

    let mut second_updated = second;
    second_updated.src = 1000;
    second_updated.edge_type = 44;
    second_updated.flags = 77;

    store.update(1, second_updated).unwrap();

    assert_eq!(store.read(0).unwrap(), first);
    assert_eq!(store.read(1).unwrap(), second_updated);

    cleanup_file(&path);
}
