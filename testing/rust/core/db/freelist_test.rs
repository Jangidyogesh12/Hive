use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;

#[test]
fn freed_page_survives_reopen() {
    let dir = temp_dir("freelist_survives_reopen");
    let mut db = HiveDb::open(&dir).unwrap();

    let mut node_ids = Vec::new();
    for _ in 0..200 {
        node_ids.push(db.create_node().unwrap());
    }

    for i in (0..200).step_by(2) {
        db.delete_node(node_ids[i]).unwrap();
    }

    db.close();

    let mut db = HiveDb::open(&dir).unwrap();

    let nodes = db.scan_nodes().unwrap();
    assert_eq!(
        nodes.len(),
        100,
        "should have 100 remaining nodes after reopen"
    );

    let new_id = db.create_node().unwrap();
    assert!(new_id != u64::MAX);

    let nodes = db.scan_nodes().unwrap();
    assert_eq!(
        nodes.len(),
        101,
        "should have 101 nodes after creating one more"
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn deleted_record_space_is_reusable() {
    let dir = temp_dir("freelist_record_reuse");
    let mut db = HiveDb::open(&dir).unwrap();

    let id1 = db.create_node().unwrap();
    let id2 = db.create_node().unwrap();

    db.delete_node(id1).unwrap();

    let id3 = db.create_node().unwrap();

    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);

    let nodes = db.scan_nodes().unwrap();
    assert_eq!(nodes.len(), 2);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn page_compaction_reclaims_space() {
    let dir = temp_dir("freelist_compaction");
    let mut db = HiveDb::open(&dir).unwrap();

    let mut node_ids = Vec::new();
    for _ in 0..50 {
        node_ids.push(db.create_node().unwrap());
    }

    for i in (0..50).step_by(2) {
        db.delete_node(node_ids[i]).unwrap();
    }

    let nodes = db.scan_nodes().unwrap();
    assert_eq!(nodes.len(), 25);

    for (_, node) in &nodes {
        assert_ne!(node.id, 0);
    }

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn storage_integrity_after_multiple_reopen() {
    let dir = temp_dir("freelist_multi_reopen");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        for _ in 0..50 {
            let id = db.create_node().unwrap();
            db.set_node_property(id, "prop", &crate::value::Value::Integer(42))
                .unwrap();
        }
        let nodes = db.scan_nodes().unwrap();
        for (id, _) in nodes.iter().take(25) {
            db.delete_node(*id).unwrap();
        }
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let nodes = db.scan_nodes().unwrap();
        assert_eq!(nodes.len(), 25, "should have 25 nodes after first reopen");

        for _ in 0..30 {
            let id = db.create_node().unwrap();
            db.set_node_property(id, "prop", &crate::value::Value::Integer(100))
                .unwrap();
        }

        let nodes = db.scan_nodes().unwrap();
        assert_eq!(nodes.len(), 55, "should have 55 nodes after adding more");

        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let nodes = db.scan_nodes().unwrap();
        assert_eq!(nodes.len(), 55, "should have 55 nodes after final reopen");

        for (_id, _node) in &nodes {
            let val = db.get_node_property(*_id, "prop").unwrap();
            match val {
                crate::value::Value::Integer(v) => assert!(v == 42 || v == 100),
                _ => panic!("unexpected property type"),
            }
        }

        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn edge_delete_and_reopen_preserves_freelist() {
    let dir = temp_dir("freelist_edge_reopen");
    let mut db = HiveDb::open(&dir).unwrap();

    let n1 = db.create_node().unwrap();
    let n2 = db.create_node().unwrap();
    let n3 = db.create_node().unwrap();

    let e1 = db.create_edge(n1, n2).unwrap();
    let _e2 = db.create_edge(n2, n3).unwrap();

    db.delete_edge(e1).unwrap();

    db.close();

    let mut db = HiveDb::open(&dir).unwrap();

    let e3 = db.create_edge(n1, n3).unwrap();
    assert!(e3 != u64::MAX);

    let edges = db.scan_edges().unwrap();
    assert_eq!(
        edges.len(),
        2,
        "should have 2 edges after delete and reopen"
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn page_reuse_after_compaction() {
    let dir = temp_dir("freelist_page_reuse");
    let mut db = HiveDb::open(&dir).unwrap();

    let mut ids = Vec::new();
    for _ in 0..300 {
        ids.push(db.create_node().unwrap());
    }

    for i in (0..300).step_by(2) {
        db.delete_node(ids[i]).unwrap();
    }

    let mut new_ids = Vec::new();
    for _ in 0..100 {
        new_ids.push(db.create_node().unwrap());
    }

    let nodes = db.scan_nodes().unwrap();
    assert_eq!(nodes.len(), 250, "should have 250 nodes total");

    let mut all_ids: Vec<_> = nodes.iter().map(|(id, _)| *id).collect();
    all_ids.sort();
    all_ids.dedup();
    assert_eq!(all_ids.len(), 250, "all node IDs should be unique");

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn rollback_on_allocated_page_returns_to_freelist() {
    let dir = temp_dir("freelist_rollback");
    let mut db = HiveDb::open(&dir).unwrap();

    let _id1 = db.create_node().unwrap();

    {
        let mut tx = db.begin().unwrap();
        let _id2 = tx.create_node().unwrap();
        let _id3 = tx.create_node().unwrap();
        tx.rollback().unwrap();
    }

    let id4 = db.create_node().unwrap();
    let id5 = db.create_node().unwrap();

    assert_ne!(id4, id5);

    let nodes = db.scan_nodes().unwrap();
    assert_eq!(
        nodes.len(),
        3,
        "should have 3 nodes (1 original + 2 new after rollback)"
    );

    db.close();
    cleanup_dir(&dir);
}
