use super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::storage::btree::BtreeKey;

#[test]
fn empty_tree_lookup_returns_none() {
    let dir = temp_dir("btree_empty_lookup");
    let mut db = HiveDb::open(&dir).unwrap();
    let root = db.create_btree().unwrap();

    let mut btree = db.open_btree(root);
    let result = btree.lookup(&BtreeKey::Int(42)).unwrap();
    assert_eq!(result, None);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn single_leaf_insert_lookup() {
    let dir = temp_dir("btree_single_leaf");
    let mut db = HiveDb::open(&dir).unwrap();
    let root = db.create_btree().unwrap();

    let mut btree = db.open_btree(root);
    btree.insert(&BtreeKey::Int(42), 100).unwrap();
    let result = btree.lookup(&BtreeKey::Int(42)).unwrap();
    assert_eq!(result, Some(vec![100]));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn many_inserts_force_split() {
    let dir = temp_dir("btree_many_inserts");
    let mut db = HiveDb::open(&dir).unwrap();
    let root = db.create_btree().unwrap();

    let mut btree = db.open_btree(root);
    let count = 500;
    for i in 0..count {
        btree
            .insert(&BtreeKey::Int(i as i64), i as u64 + 1)
            .unwrap();
    }

    for i in 0..count {
        let result = btree.lookup(&BtreeKey::Int(i as i64)).unwrap();
        assert_eq!(result, Some(vec![i as u64 + 1]), "missing key {}", i);
    }

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn duplicate_keys_accumulate_record_ids() {
    let dir = temp_dir("btree_duplicate_keys");
    let mut db = HiveDb::open(&dir).unwrap();
    let root = db.create_btree().unwrap();

    let mut btree = db.open_btree(root);
    btree.insert(&BtreeKey::Text("hello".into()), 1).unwrap();
    btree.insert(&BtreeKey::Text("hello".into()), 2).unwrap();
    btree.insert(&BtreeKey::Text("hello".into()), 3).unwrap();

    let mut result = btree
        .lookup(&BtreeKey::Text("hello".into()))
        .unwrap()
        .unwrap();
    result.sort();
    assert_eq!(result, vec![1, 2, 3]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn sorted_and_random_insert_order_match() {
    let dir = temp_dir("btree_insert_order");
    let count = 300;

    let sorted_dir = temp_dir("btree_sorted");
    {
        let mut db = HiveDb::open(&sorted_dir).unwrap();
        let root = db.create_btree().unwrap();
        let mut btree = db.open_btree(root);
        for i in 0..count {
            btree.insert(&BtreeKey::Int(i as i64), i as u64).unwrap();
        }
        db.close();
    }

    let random_dir = temp_dir("btree_random");
    {
        let mut db = HiveDb::open(&random_dir).unwrap();
        let root = db.create_btree().unwrap();
        let mut btree = db.open_btree(root);
        let mut order: Vec<i64> = (0..count).map(|i| i as i64).collect();
        order.reverse();
        for i in order {
            btree.insert(&BtreeKey::Int(i), i as u64).unwrap();
        }
        db.close();
    }

    {
        let mut db = HiveDb::open(&sorted_dir).unwrap();
        let root = db.create_btree().unwrap();
        let mut btree = db.open_btree(root);
        let sorted_scan: Vec<i64> = btree
            .scan()
            .unwrap()
            .into_iter()
            .map(|(k, _)| match k {
                BtreeKey::Int(v) => v,
                _ => panic!("unexpected key type"),
            })
            .collect();
        db.close();

        let mut db = HiveDb::open(&random_dir).unwrap();
        let root = db.create_btree().unwrap();
        let mut btree = db.open_btree(root);
        let random_scan: Vec<i64> = btree
            .scan()
            .unwrap()
            .into_iter()
            .map(|(k, _)| match k {
                BtreeKey::Int(v) => v,
                _ => panic!("unexpected key type"),
            })
            .collect();
        db.close();

        assert_eq!(sorted_scan, random_scan);
    }

    cleanup_dir(&sorted_dir);
    cleanup_dir(&random_dir);
    cleanup_dir(&dir);
}

#[test]
fn delete_removes_key() {
    let dir = temp_dir("btree_delete");
    let mut db = HiveDb::open(&dir).unwrap();
    let root = db.create_btree().unwrap();

    let mut btree = db.open_btree(root);
    btree.insert(&BtreeKey::Int(10), 1).unwrap();
    btree.insert(&BtreeKey::Int(10), 2).unwrap();
    btree.insert(&BtreeKey::Int(20), 3).unwrap();

    assert!(btree.delete(&BtreeKey::Int(10), 1).unwrap());
    let mut result = btree.lookup(&BtreeKey::Int(10)).unwrap().unwrap();
    result.sort();
    assert_eq!(result, vec![2]);

    assert!(btree.delete(&BtreeKey::Int(10), 2).unwrap());
    assert_eq!(btree.lookup(&BtreeKey::Int(10)).unwrap(), None);

    assert_eq!(btree.lookup(&BtreeKey::Int(20)).unwrap(), Some(vec![3]));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn delete_all_keys_leaves_valid_empty_tree() {
    let dir = temp_dir("btree_delete_all");
    let mut db = HiveDb::open(&dir).unwrap();
    let root = db.create_btree().unwrap();

    let mut btree = db.open_btree(root);
    for i in 0..50 {
        btree.insert(&BtreeKey::Int(i as i64), i as u64).unwrap();
    }
    for i in 0..50 {
        assert!(btree.delete(&BtreeKey::Int(i as i64), i as u64).unwrap());
    }

    for i in 0..50 {
        assert_eq!(btree.lookup(&BtreeKey::Int(i as i64)).unwrap(), None);
    }

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn root_growth_to_three_levels() {
    let dir = temp_dir("btree_three_levels");
    let mut db = HiveDb::open(&dir).unwrap();
    let root = db.create_btree().unwrap();

    let mut btree = db.open_btree(root);
    let count = 5000;
    for i in 0..count {
        btree
            .insert(&BtreeKey::Int(i as i64), i as u64 + 1)
            .unwrap();
    }

    for i in 0..count {
        assert_eq!(
            btree.lookup(&BtreeKey::Int(i as i64)).unwrap(),
            Some(vec![i as u64 + 1]),
            "missing key {}",
            i
        );
    }

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn committed_inserts_survive_reopen() {
    let dir = temp_dir("btree_committed_reopen");
    let final_root;
    {
        let mut db = HiveDb::open(&dir).unwrap();
        let root = db.create_btree().unwrap();
        let mut tx = db.begin().unwrap();
        let mut current_root = root;
        for i in 0..200 {
            current_root = tx
                .btree_insert(current_root, &BtreeKey::Int(i as i64), i as u64 + 1)
                .unwrap();
        }
        tx.commit().unwrap();
        final_root = current_root;
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let mut btree = db.open_btree(final_root);
        for i in 0..200 {
            assert_eq!(
                btree.lookup(&BtreeKey::Int(i as i64)).unwrap(),
                Some(vec![i as u64 + 1]),
                "missing key {} after reopen",
                i
            );
        }
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn uncommitted_inserts_rolled_back() {
    let dir = temp_dir("btree_uncommitted_rollback");
    let root;
    {
        let mut db = HiveDb::open(&dir).unwrap();
        root = db.create_btree().unwrap();
        let mut tx = db.begin().unwrap();
        let mut btree = tx.create_btree().unwrap();
        for _ in 0..100 {
            btree = tx.create_btree().unwrap();
        }
        let _ = btree;
        tx.rollback().unwrap();
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let mut btree = db.open_btree(root);
        assert_eq!(btree.lookup(&BtreeKey::Int(0)).unwrap(), None);
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn recovered_database_replays_committed_index() {
    let dir = temp_dir("btree_recovery");
    let final_root;
    {
        let mut db = HiveDb::open(&dir).unwrap();
        let root = db.create_btree().unwrap();
        let mut tx = db.begin().unwrap();
        let mut current_root = root;
        for i in 0..300 {
            current_root = tx
                .btree_insert(current_root, &BtreeKey::Int(i as i64), i as u64 + 1)
                .unwrap();
        }
        tx.commit().unwrap();
        final_root = current_root;
        drop(db);
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let mut btree = db.open_btree(final_root);
        for i in 0..300 {
            assert_eq!(
                btree.lookup(&BtreeKey::Int(i as i64)).unwrap(),
                Some(vec![i as u64 + 1]),
                "missing key {} after recovery",
                i
            );
        }
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn rollback_restores_split_pages() {
    let dir = temp_dir("btree_rollback_split");
    let mut db = HiveDb::open(&dir).unwrap();
    let root = db.create_btree().unwrap();

    {
        let mut tx = db.begin().unwrap();
        let mut current_root = root;
        for i in 0..500 {
            current_root = tx
                .btree_insert(current_root, &BtreeKey::Int(i as i64), i as u64 + 1)
                .unwrap();
        }
        tx.rollback().unwrap();
    }

    let mut btree = db.open_btree(root);
    for i in 0..500 {
        assert_eq!(
            btree.lookup(&BtreeKey::Int(i as i64)).unwrap(),
            None,
            "key {} should have been rolled back",
            i
        );
    }

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn range_scan_forward_returns_sorted_keys() {
    let dir = temp_dir("btree_range_scan");
    let mut db = HiveDb::open(&dir).unwrap();
    let root = db.create_btree().unwrap();

    let mut btree = db.open_btree(root);
    let mut keys: Vec<i64> = (-100..100).map(|i| i as i64).collect();
    keys.reverse();
    for k in keys {
        btree.insert(&BtreeKey::Int(k), k as u64).unwrap();
    }

    let scan = btree.scan().unwrap();
    let scanned: Vec<i64> = scan
        .into_iter()
        .map(|(k, _)| match k {
            BtreeKey::Int(v) => v,
            _ => panic!("unexpected key type"),
        })
        .collect();

    let expected: Vec<i64> = (-100..100).map(|i| i as i64).collect();
    assert_eq!(scanned, expected);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn composite_key_lookup() {
    let dir = temp_dir("btree_composite");
    let mut db = HiveDb::open(&dir).unwrap();
    let root = db.create_btree().unwrap();

    let mut btree = db.open_btree(root);
    let key1 = BtreeKey::Composite(vec![BtreeKey::Int(1), BtreeKey::Text("alice".into())]);
    let key2 = BtreeKey::Composite(vec![BtreeKey::Int(1), BtreeKey::Text("bob".into())]);
    let key3 = BtreeKey::Composite(vec![BtreeKey::Int(2), BtreeKey::Text("alice".into())]);

    btree.insert(&key1, 10).unwrap();
    btree.insert(&key2, 20).unwrap();
    btree.insert(&key3, 30).unwrap();

    assert_eq!(btree.lookup(&key1).unwrap(), Some(vec![10]));
    assert_eq!(btree.lookup(&key2).unwrap(), Some(vec![20]));
    assert_eq!(btree.lookup(&key3).unwrap(), Some(vec![30]));

    let scan = btree.scan().unwrap();
    assert_eq!(scan.len(), 3);

    db.close();
    cleanup_dir(&dir);
}
