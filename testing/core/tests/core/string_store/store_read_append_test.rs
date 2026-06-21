// Tests StringStore append/read behavior with variable-length strings.
use super::super::utils::utils::{cleanup_file, temp_file};
use crate::errors::DbError;
use crate::store::string_store::StringStore;

#[test]
// Verifies a single appended string can be read back by its offset.
fn append_then_read_returns_same_string() {
    let path = temp_file("string_append_then_read_returns_same_string");
    let mut store = StringStore::open(&path).unwrap();

    let offset = store.append("Alice").unwrap();
    let got = store.read(offset).unwrap();

    assert_eq!(got, "Alice");

    cleanup_file(&path);
}

#[test]
// Verifies multiple strings of different lengths can be read back correctly.
fn append_multiple_strings_keeps_data() {
    let path = temp_file("string_append_multiple_strings_keeps_data");
    let mut store = StringStore::open(&path).unwrap();

    let o1 = store.append("Alice").unwrap();
    let o2 = store.append("Bob").unwrap();
    let o3 = store.append("charlie@email.com").unwrap();

    assert_eq!(store.read(o1).unwrap(), "Alice");
    assert_eq!(store.read(o2).unwrap(), "Bob");
    assert_eq!(store.read(o3).unwrap(), "charlie@email.com");

    cleanup_file(&path);
}

#[test]
// Verifies offsets are distinct and correct for sequential appends.
fn append_returns_increasing_offsets() {
    let path = temp_file("string_append_returns_increasing_offsets");
    let mut store = StringStore::open(&path).unwrap();

    let o1 = store.append("short").unwrap();
    let o2 = store.append("a bit longer string").unwrap();

    assert!(o2 > o1);

    cleanup_file(&path);
}

#[test]
// Verifies empty string can be appended and read back.
fn append_empty_string_roundtrip() {
    let path = temp_file("string_append_empty_string_roundtrip");
    let mut store = StringStore::open(&path).unwrap();

    let offset = store.append("").unwrap();
    let got = store.read(offset).unwrap();

    assert_eq!(got, "");

    cleanup_file(&path);
}

#[test]
// Verifies reading from an invalid offset returns ReadError.
fn read_invalid_offset_returns_read_error() {
    let path = temp_file("string_read_invalid_offset_returns_read_error");
    let mut store = StringStore::open(&path).unwrap();

    store.append("test").unwrap();

    let result = store.read(999);
    assert!(matches!(result, Err(DbError::ReadError)));

    cleanup_file(&path);
}
