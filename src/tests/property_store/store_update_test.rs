// Tests PropertyStore update behavior and record isolation guarantees.
use super::super::utils::utils::{cleanup_file, temp_file};
use crate::store::property_record::PropertyRecord;
use crate::store::property_store::PropertyStore;

#[test]
// Verifies update replaces data at the targeted record index.
fn update_overwrites_existing_record() {
    let path = temp_file("property_update_overwrites_existing_record");
    let mut store = PropertyStore::open(&path).unwrap();

    let mut original = PropertyRecord::new(1);
    original.flags = 10;
    store.append(original).unwrap();

    let mut updated = PropertyRecord::new(1);
    updated.key_hash = 55;
    updated.value_type = 2;
    updated.value_inline = [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1];
    updated.next_property = 77;
    updated.flags = 42;
    updated.reserved = 7;

    store.update(0, updated).unwrap();

    let got = store.read(0).unwrap();
    assert_eq!(got, updated);

    cleanup_file(&path);
}

#[test]
// Verifies updating one record does not alter neighboring records.
fn update_keeps_other_records_unchanged() {
    let path = temp_file("property_update_keeps_other_records_unchanged");
    let mut store = PropertyStore::open(&path).unwrap();

    let mut first = PropertyRecord::new(1);
    first.flags = 11;

    let mut second = PropertyRecord::new(2);
    second.flags = 22;

    store.append(first).unwrap();
    store.append(second).unwrap();

    let mut second_updated = second;
    second_updated.key_hash = 1234;
    second_updated.flags = 99;

    store.update(1, second_updated).unwrap();

    assert_eq!(store.read(0).unwrap(), first);
    assert_eq!(store.read(1).unwrap(), second_updated);

    cleanup_file(&path);
}
