// Tests EdgeStore open behavior and file creation/error handling.
use super::super::utils::utils::{cleanup_file, temp_file};
use crate::errors::DbError;
use crate::store::edge::store::EdgeStore;

#[test]
// Verifies opening a missing path creates the backing file.
fn open_creates_new_file() {
    let path = temp_file("edge_open_creates_new_file");
    assert!(!path.exists());

    let result = EdgeStore::open(&path);

    assert!(result.is_ok());
    assert!(path.exists());
    cleanup_file(&path);
}

#[test]
// Verifies opening a directory path returns FileOpenError.
fn open_invalid_path_returns_file_open_error() {
    let dir_path = std::env::temp_dir();

    let result = EdgeStore::open(&dir_path);

    assert!(matches!(result, Err(DbError::FileOpenError)));
}
