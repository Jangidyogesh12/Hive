// Tests LabelStore get_or_create behavior: deduplication, ID assignment, and sequential IDs.
use super::super::utils::utils::{cleanup_file, temp_file};
use crate::store::label_store::LabelStore;

#[test]
fn get_or_create_returns_one_for_first_label() {
    let path = temp_file("label_get_or_create_returns_one_for_first_label");
    let mut store = LabelStore::open(&path).unwrap();

    let id = store.get_or_create("User").unwrap();

    assert_eq!(id, 1);
    cleanup_file(&path);
}

#[test]
fn get_or_create_returns_sequential_ids() {
    let path = temp_file("label_get_or_create_returns_sequential_ids");
    let mut store = LabelStore::open(&path).unwrap();

    let id1 = store.get_or_create("User").unwrap();
    let id2 = store.get_or_create("Product").unwrap();
    let id3 = store.get_or_create("Order").unwrap();

    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(id3, 3);
    cleanup_file(&path);
}

#[test]
fn get_or_create_returns_same_id_for_duplicate_label() {
    let path = temp_file("label_get_or_create_returns_same_id_for_duplicate_label");
    let mut store = LabelStore::open(&path).unwrap();

    let first = store.get_or_create("User").unwrap();
    let second = store.get_or_create("User").unwrap();

    assert_eq!(first, second);
    cleanup_file(&path);
}

#[test]
fn get_or_create_does_not_increment_next_id_on_duplicate() {
    let path = temp_file("label_get_or_create_does_not_increment_next_id_on_duplicate");
    let mut store = LabelStore::open(&path).unwrap();

    store.get_or_create("User").unwrap();
    store.get_or_create("User").unwrap();
    store.get_or_create("User").unwrap();

    assert_eq!(store.next_id, 2);
    cleanup_file(&path);
}

#[test]
fn get_or_create_handles_many_labels() {
    let path = temp_file("label_get_or_create_handles_many_labels");
    let mut store = LabelStore::open(&path).unwrap();

    for i in 1..=100 {
        let label = format!("Label{}", i);
        let id = store.get_or_create(&label).unwrap();
        assert_eq!(id, i);
    }

    assert_eq!(store.next_id, 101);
    cleanup_file(&path);
}

#[test]
fn get_or_create_populates_label_to_id_map() {
    let path = temp_file("label_get_or_create_populates_label_to_id_map");
    let mut store = LabelStore::open(&path).unwrap();

    store.get_or_create("User").unwrap();
    store.get_or_create("Product").unwrap();

    assert_eq!(store.label_to_id.len(), 2);
    assert_eq!(*store.label_to_id.get("User").unwrap(), 1);
    assert_eq!(*store.label_to_id.get("Product").unwrap(), 2);
    cleanup_file(&path);
}

#[test]
fn get_or_create_populates_id_to_label_map() {
    let path = temp_file("label_get_or_create_populates_id_to_label_map");
    let mut store = LabelStore::open(&path).unwrap();

    store.get_or_create("User").unwrap();
    store.get_or_create("Product").unwrap();

    assert_eq!(store.id_to_label.len(), 2);
    assert_eq!(store.id_to_label.get(&1).unwrap(), "User");
    assert_eq!(store.id_to_label.get(&2).unwrap(), "Product");
    cleanup_file(&path);
}
