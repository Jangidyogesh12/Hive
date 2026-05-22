use super::super::utils::utils::{cleanup_file, temp_file};
use crate::store::free_list::FreeList;

#[test]
fn pop_empty_returns_none() {
    let path = temp_file("fl_pop_empty");
    let mut fl = FreeList::open(&path).unwrap();

    assert_eq!(fl.pop(), None);
    cleanup_file(&path);
}

#[test]
fn push_then_pop_returns_same_id() {
    let path = temp_file("fl_push_pop_same");
    let mut fl = FreeList::open(&path).unwrap();

    fl.push(42).unwrap();
    assert_eq!(fl.len(), 1);

    let popped = fl.pop().unwrap();
    assert_eq!(popped, 42);
    assert!(fl.is_empty());
    cleanup_file(&path);
}

#[test]
fn push_multiple_then_pop_in_reverse_order() {
    let path = temp_file("fl_push_multi_pop_reverse");
    let mut fl = FreeList::open(&path).unwrap();

    fl.push(100).unwrap();
    fl.push(200).unwrap();
    fl.push(300).unwrap();

    assert_eq!(fl.pop().unwrap(), 300);
    assert_eq!(fl.pop().unwrap(), 200);
    assert_eq!(fl.pop().unwrap(), 100);
    assert!(fl.is_empty());
    cleanup_file(&path);
}

#[test]
fn push_pop_push_handles_interleaving() {
    let path = temp_file("fl_interleave");
    let mut fl = FreeList::open(&path).unwrap();

    fl.push(1).unwrap();
    fl.push(2).unwrap();
    assert_eq!(fl.pop().unwrap(), 2);

    fl.push(3).unwrap();
    assert_eq!(fl.pop().unwrap(), 3);
    assert_eq!(fl.pop().unwrap(), 1);
    assert!(fl.is_empty());
    cleanup_file(&path);
}

#[test]
fn persistence_after_pop_survives_reopen() {
    let path = temp_file("fl_persist_after_pop");
    {
        let mut fl = FreeList::open(&path).unwrap();
        fl.push(7).unwrap();
        fl.push(8).unwrap();
        fl.push(9).unwrap();
        fl.pop(); // remove 9, leaving [7, 8]
    }

    let mut fl = FreeList::open(&path).unwrap();
    assert_eq!(fl.len(), 2);
    assert_eq!(fl.pop().unwrap(), 8);
    assert_eq!(fl.pop().unwrap(), 7);
    assert!(fl.is_empty());
    cleanup_file(&path);
}

#[test]
fn persistence_after_push_survives_reopen() {
    let path = temp_file("fl_persist_after_push");
    {
        let mut fl = FreeList::open(&path).unwrap();
        fl.push(55).unwrap();
        fl.push(66).unwrap();
    }

    let mut fl = FreeList::open(&path).unwrap();
    assert_eq!(fl.len(), 2);
    assert_eq!(fl.pop().unwrap(), 66);
    assert_eq!(fl.pop().unwrap(), 55);
    cleanup_file(&path);
}

#[test]
fn drain_all_then_reopen_is_empty() {
    let path = temp_file("fl_drain_reopen_empty");
    {
        let mut fl = FreeList::open(&path).unwrap();
        fl.push(10).unwrap();
        fl.push(20).unwrap();
        fl.pop();
        fl.pop();
    }

    let fl = FreeList::open(&path).unwrap();
    assert!(fl.is_empty());
    assert_eq!(fl.len(), 0);
    cleanup_file(&path);
}

#[test]
fn len_and_is_empty_reflect_state() {
    let path = temp_file("fl_len_is_empty");
    let mut fl = FreeList::open(&path).unwrap();

    assert!(fl.is_empty());
    assert_eq!(fl.len(), 0);

    fl.push(1).unwrap();
    assert!(!fl.is_empty());
    assert_eq!(fl.len(), 1);

    fl.push(2).unwrap();
    assert_eq!(fl.len(), 2);

    fl.pop();
    assert_eq!(fl.len(), 1);

    fl.pop();
    assert!(fl.is_empty());
    assert_eq!(fl.len(), 0);
    cleanup_file(&path);
}

#[test]
fn push_any_u64_id_is_preserved() {
    let path = temp_file("fl_u64_range");
    let mut fl = FreeList::open(&path).unwrap();

    fl.push(0).unwrap();
    fl.push(u64::MAX).unwrap();
    fl.push(1_000_000_000).unwrap();

    let mut values = Vec::new();
    values.push(fl.pop().unwrap());
    values.push(fl.pop().unwrap());
    values.push(fl.pop().unwrap());
    values.sort();
    assert_eq!(values, vec![0, 1_000_000_000, u64::MAX]);
    cleanup_file(&path);
}
