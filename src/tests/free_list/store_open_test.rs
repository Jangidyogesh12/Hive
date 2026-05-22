use super::super::utils::utils::{cleanup_file, temp_file};
use crate::errors::DbError;
use crate::store::free_list::FreeList;

#[test]
fn open_creates_new_file() {
    let path = temp_file("fl_open_creates_new_file");
    assert!(!path.exists());

    let result = FreeList::open(&path);

    assert!(result.is_ok());
    assert!(path.exists());
    cleanup_file(&path);
}

#[test]
fn open_empty_returns_empty_list() {
    let path = temp_file("fl_open_empty");
    let fl = FreeList::open(&path).unwrap();

    assert!(fl.is_empty());
    assert_eq!(fl.len(), 0);
    cleanup_file(&path);
}

#[test]
fn open_invalid_path_returns_error() {
    let dir_path = std::env::temp_dir();

    let result = FreeList::open(&dir_path);

    assert!(matches!(result, Err(DbError::FileOpenError)));
}

#[test]
fn open_preexisting_file_loads_entries() {
    let path = temp_file("fl_open_preexisting");
    {
        let mut fl = FreeList::open(&path).unwrap();
        fl.push(10).unwrap();
        fl.push(20).unwrap();
        fl.push(30).unwrap();
    }

    let fl = FreeList::open(&path).unwrap();
    assert_eq!(fl.len(), 3);

    let mut values: Vec<u64> = Vec::new();
    let mut fl_mut = fl;
    values.push(fl_mut.pop().unwrap());
    values.push(fl_mut.pop().unwrap());
    values.push(fl_mut.pop().unwrap());
    values.sort();
    assert_eq!(values, vec![10, 20, 30]);
    cleanup_file(&path);
}
