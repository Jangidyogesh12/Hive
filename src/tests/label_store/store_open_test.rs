// Tests LabelStore open behavior and file creation/error handling.
use super::super::utils::utils::{cleanup_file, temp_file};
use crate::errors::DbError;
use crate::store::label_store::LabelStore;

#[test]
fn open_creates_new_file() {
    let path = temp_file("label_open_creates_new_file");
    assert!(!path.exists());

    let result = LabelStore::open(&path);

    assert!(result.is_ok());
    assert!(path.exists());
    cleanup_file(&path);
}

#[test]
fn open_invalid_path_returns_file_open_error() {
    let dir_path = std::env::temp_dir();

    let result = LabelStore::open(&dir_path);

    assert!(matches!(result, Err(DbError::FileOpenError)));
}

#[test]
fn open_initializes_next_id_to_one() {
    let path = temp_file("label_open_initializes_next_id_to_one");
    let store = LabelStore::open(&path).unwrap();

    assert_eq!(store.next_id, 1);
    cleanup_file(&path);
}

#[test]
fn open_initializes_empty_maps() {
    let path = temp_file("label_open_initializes_empty_maps");
    let store = LabelStore::open(&path).unwrap();

    assert!(store.label_to_id.is_empty());
    assert!(store.id_to_label.is_empty());
    cleanup_file(&path);
}
