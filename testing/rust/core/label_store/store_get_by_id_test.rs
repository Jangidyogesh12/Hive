// Tests LabelStore get_by_id behavior: reverse lookup and missing ID handling.
use super::super::utils::utils::{cleanup_file, temp_file};
use crate::store::label_store::LabelStore;

#[test]
fn get_by_id_returns_label_string() {
    let path = temp_file("label_get_by_id_returns_label_string");
    let mut store = LabelStore::open(&path).unwrap();

    store.get_or_create("User").unwrap();

    assert_eq!(store.get_by_id(1), Some("User"));
    cleanup_file(&path);
}

#[test]
fn get_by_id_returns_none_for_missing_id() {
    let path = temp_file("label_get_by_id_returns_none_for_missing_id");
    let store = LabelStore::open(&path).unwrap();

    assert_eq!(store.get_by_id(999), None);
    cleanup_file(&path);
}

#[test]
fn get_by_id_returns_none_for_id_zero() {
    let path = temp_file("label_get_by_id_returns_none_for_id_zero");
    let store = LabelStore::open(&path).unwrap();

    assert_eq!(store.get_by_id(0), None);
    cleanup_file(&path);
}

#[test]
fn get_by_id_returns_correct_label_for_multiple_entries() {
    let path = temp_file("label_get_by_id_returns_correct_label_for_multiple_entries");
    let mut store = LabelStore::open(&path).unwrap();

    store.get_or_create("User").unwrap();
    store.get_or_create("Product").unwrap();
    store.get_or_create("Order").unwrap();

    assert_eq!(store.get_by_id(1), Some("User"));
    assert_eq!(store.get_by_id(2), Some("Product"));
    assert_eq!(store.get_by_id(3), Some("Order"));
    cleanup_file(&path);
}

#[test]
fn get_by_id_returns_none_after_only_some_labels_created() {
    let path = temp_file("label_get_by_id_returns_none_after_only_some_labels_created");
    let mut store = LabelStore::open(&path).unwrap();

    store.get_or_create("User").unwrap();

    assert_eq!(store.get_by_id(1), Some("User"));
    assert_eq!(store.get_by_id(2), None);
    cleanup_file(&path);
}
