use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;

#[test]
fn create_edge_returns_valid_id() {
    let dir = temp_dir("create_edge_valid_id");
    let mut db = HiveDb::open(&dir).unwrap();

    let n1 = db.create_node().unwrap();
    let n2 = db.create_node().unwrap();
    let edge_id = db.create_edge(n1, n2).unwrap();

    assert!(edge_id != u64::MAX);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_and_read_edge() {
    let dir = temp_dir("create_and_read_edge");
    let mut db = HiveDb::open(&dir).unwrap();

    let n1 = db.create_node().unwrap();
    let n2 = db.create_node().unwrap();
    let edge_id = db.create_edge(n1, n2).unwrap();

    let edge = db.get_edge(edge_id).unwrap();
    assert_eq!(edge.id, 1);
    assert_eq!(edge.src, n1);
    assert_eq!(edge.dst, n2);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_multiple_edges_returns_distinct_ids() {
    let dir = temp_dir("create_multiple_edges");
    let mut db = HiveDb::open(&dir).unwrap();

    let n1 = db.create_node().unwrap();
    let n2 = db.create_node().unwrap();
    let n3 = db.create_node().unwrap();

    let e1 = db.create_edge(n1, n2).unwrap();
    let e2 = db.create_edge(n2, n3).unwrap();
    let e3 = db.create_edge(n3, n1).unwrap();

    assert_ne!(e1, e2);
    assert_ne!(e2, e3);
    assert_ne!(e1, e3);

    let edge1 = db.get_edge(e1).unwrap();
    let edge2 = db.get_edge(e2).unwrap();
    let edge3 = db.get_edge(e3).unwrap();

    assert_eq!(edge1.src, n1);
    assert_eq!(edge1.dst, n2);
    assert_eq!(edge2.src, n2);
    assert_eq!(edge2.dst, n3);
    assert_eq!(edge3.src, n3);
    assert_eq!(edge3.dst, n1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_edge_persists_after_reopen() {
    let dir = temp_dir("create_edge_persists");
    let edge_id;

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let n1 = db.create_node().unwrap();
        let n2 = db.create_node().unwrap();
        edge_id = db.create_edge(n1, n2).unwrap();
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let edge = db.get_edge(edge_id).unwrap();
        assert_eq!(edge.id, 1);
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn get_edge_returns_error_for_invalid_id() {
    let dir = temp_dir("get_edge_invalid");
    let mut db = HiveDb::open(&dir).unwrap();

    let result = db.get_edge(u64::MAX);
    assert!(result.is_err());

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn scan_edges_across_multiple_pages() {
    let dir = temp_dir("scan_multi_page_edges");
    let mut db = HiveDb::open(&dir).unwrap();

    let n1 = db.create_node().unwrap();
    let n2 = db.create_node().unwrap();

    let count = 200;
    let mut ids = Vec::with_capacity(count);
    for _ in 0..count {
        ids.push(db.create_edge(n1, n2).unwrap());
    }

    let edges = db.scan_edges().unwrap();
    assert_eq!(edges.len(), count, "scan should find all {} edges", count);

    for (idx, (edge_id, edge)) in edges.iter().enumerate() {
        assert_eq!(edge.id, (idx + 1) as u64);
        assert_eq!(edge.src, n1);
        assert_eq!(edge.dst, n2);
        assert_ne!(*edge_id, u64::MAX);
    }

    for id in &ids {
        let edge = db.get_edge(*id).unwrap();
        assert_eq!(edge.src, n1);
        assert_eq!(edge.dst, n2);
    }

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn scan_edges_skips_deleted_slots() {
    let dir = temp_dir("scan_skips_deleted_edges");
    let mut db = HiveDb::open(&dir).unwrap();

    let n1 = db.create_node().unwrap();
    let n2 = db.create_node().unwrap();
    let n3 = db.create_node().unwrap();

    let e1 = db.create_edge(n1, n2).unwrap();
    let e2 = db.create_edge(n2, n3).unwrap();
    let e3 = db.create_edge(n3, n1).unwrap();

    db.delete_edge(e2).unwrap();

    let edges = db.scan_edges().unwrap();
    assert_eq!(edges.len(), 2, "scan should skip deleted edge");

    let ids: Vec<_> = edges.iter().map(|(id, _)| *id).collect();
    assert!(ids.contains(&e1));
    assert!(!ids.contains(&e2));
    assert!(ids.contains(&e3));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn get_edge_fails_for_deleted_record() {
    let dir = temp_dir("get_deleted_edge_fails");
    let mut db = HiveDb::open(&dir).unwrap();

    let n1 = db.create_node().unwrap();
    let n2 = db.create_node().unwrap();
    let edge_id = db.create_edge(n1, n2).unwrap();

    let edge = db.get_edge(edge_id).unwrap();
    assert_eq!(edge.id, 1);

    db.delete_edge(edge_id).unwrap();

    let result = db.get_edge(edge_id);
    assert!(result.is_err(), "point read of deleted edge should fail");

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn delete_edge_rollback_restores_record() {
    let dir = temp_dir("delete_edge_rollback");
    let mut db = HiveDb::open(&dir).unwrap();

    let n1 = db.create_node().unwrap();
    let n2 = db.create_node().unwrap();
    let n3 = db.create_node().unwrap();

    let e1 = db.create_edge(n1, n2).unwrap();
    let e2 = db.create_edge(n2, n3).unwrap();

    {
        let mut tx = db.begin().unwrap();
        tx.delete_edge(e2).unwrap();
        let edges_before_rollback = tx.scan_edges().unwrap();
        assert_eq!(edges_before_rollback.len(), 1);
        tx.rollback().unwrap();
    }

    let edge = db.get_edge(e2).unwrap();
    assert_eq!(edge.id, 2);

    let edges = db.scan_edges().unwrap();
    assert_eq!(edges.len(), 2, "rollback should restore deleted edge");

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn delete_edge_persists_after_reopen() {
    let dir = temp_dir("delete_edge_persists");
    let mut db = HiveDb::open(&dir).unwrap();

    let n1 = db.create_node().unwrap();
    let n2 = db.create_node().unwrap();
    let n3 = db.create_node().unwrap();

    let e1 = db.create_edge(n1, n2).unwrap();
    let e2 = db.create_edge(n2, n3).unwrap();
    let e3 = db.create_edge(n3, n1).unwrap();

    db.delete_edge(e2).unwrap();

    db.close();

    let mut db = HiveDb::open(&dir).unwrap();
    let edges = db.scan_edges().unwrap();
    assert_eq!(
        edges.len(),
        2,
        "deleted edge should not appear after reopen"
    );

    let ids: Vec<_> = edges.iter().map(|(id, _)| *id).collect();
    assert!(ids.contains(&e1));
    assert!(!ids.contains(&e2));
    assert!(ids.contains(&e3));

    assert!(
        db.get_edge(e2).is_err(),
        "deleted edge should not be readable"
    );

    db.close();
    cleanup_dir(&dir);
}
