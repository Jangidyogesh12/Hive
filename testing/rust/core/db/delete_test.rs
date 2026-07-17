use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::types::DELETED;

#[test]
fn delete_node_sets_deleted_flag() {
    let dir = temp_dir("delete_node_sets_deleted_flag");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let id = db.create_node("Person", vec![]).unwrap();
    db.delete_node(id).unwrap();

    let node = db.get_node(id).unwrap();
    assert!(node.flags & DELETED != 0);

    cleanup_dir(&dir);
}

#[test]
fn delete_node_returns_id() {
    let dir = temp_dir("delete_node_returns_id");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let id = db.create_node("Person", vec![]).unwrap();
    let result = db.delete_node(id).unwrap();

    assert_eq!(result, id);

    cleanup_dir(&dir);
}

#[test]
fn delete_node_idempotent() {
    let dir = temp_dir("delete_node_idempotent");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let id = db.create_node("Person", vec![]).unwrap();
    db.delete_node(id).unwrap();
    db.delete_node(id).unwrap();

    let node = db.get_node(id).unwrap();
    assert!(node.flags & DELETED != 0);

    cleanup_dir(&dir);
}

#[test]
fn delete_node_persists_across_reopen() {
    let dir = temp_dir("delete_node_persists_across_reopen");

    let id = {
        let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();
        let id = db.create_node("Person", vec![]).unwrap();
        db.delete_node(id).unwrap();
        id
    };

    let mut db2 = crate::db::hive_db::HiveDb::open(&dir).unwrap();
    let node = db2.get_node(id).unwrap();
    assert!(node.flags & DELETED != 0);

    cleanup_dir(&dir);
}

#[test]
fn delete_edge_sets_deleted_flag() {
    let dir = temp_dir("delete_edge_sets_deleted_flag");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let src = db.create_node("A", vec![]).unwrap();
    let dst = db.create_node("B", vec![]).unwrap();
    let id = db.create_edge(src, dst, "KNOWS", vec![]).unwrap();
    db.delete_edge(id).unwrap();

    let edge = db.get_edge(id).unwrap();
    assert!(edge.flags & DELETED != 0);

    cleanup_dir(&dir);
}

#[test]
fn delete_edge_returns_id() {
    let dir = temp_dir("delete_edge_returns_id");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let src = db.create_node("A", vec![]).unwrap();
    let dst = db.create_node("B", vec![]).unwrap();
    let id = db.create_edge(src, dst, "KNOWS", vec![]).unwrap();
    let result = db.delete_edge(id).unwrap();

    assert_eq!(result, id);

    cleanup_dir(&dir);
}

#[test]
fn delete_edge_persists_across_reopen() {
    let dir = temp_dir("delete_edge_persists_across_reopen");

    let id = {
        let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();
        let src = db.create_node("A", vec![]).unwrap();
        let dst = db.create_node("B", vec![]).unwrap();
        let id = db.create_edge(src, dst, "KNOWS", vec![]).unwrap();
        db.delete_edge(id).unwrap();
        id
    };

    let mut db2 = crate::db::hive_db::HiveDb::open(&dir).unwrap();
    let edge = db2.get_edge(id).unwrap();
    assert!(edge.flags & DELETED != 0);

    cleanup_dir(&dir);
}

#[test]
fn delete_edge_unlinks_head_from_out_chain() {
    let dir = temp_dir("delete_edge_unlinks_head_from_out_chain");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let c = db.create_node("C", vec![]).unwrap();

    let _e0 = db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    let e1 = db.create_edge(a, c, "KNOWS", vec![]).unwrap();

    db.delete_edge(e1).unwrap();

    let neighbors = db.get_out_neighbors(a).unwrap();
    assert_eq!(neighbors, vec![b]);

    cleanup_dir(&dir);
}

#[test]
fn delete_edge_unlinks_head_from_in_chain() {
    let dir = temp_dir("delete_edge_unlinks_head_from_in_chain");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let c = db.create_node("C", vec![]).unwrap();

    let _e0 = db.create_edge(a, c, "KNOWS", vec![]).unwrap();
    let e1 = db.create_edge(b, c, "KNOWS", vec![]).unwrap();

    db.delete_edge(e1).unwrap();

    let neighbors = db.get_in_neighbors(c).unwrap();
    assert_eq!(neighbors, vec![a]);

    cleanup_dir(&dir);
}

#[test]
fn delete_edge_unlinks_middle_from_out_chain() {
    let dir = temp_dir("delete_edge_unlinks_middle_from_out_chain");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let c = db.create_node("C", vec![]).unwrap();
    let d = db.create_node("D", vec![]).unwrap();

    let _e0 = db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    let e1 = db.create_edge(a, c, "KNOWS", vec![]).unwrap();
    let _e2 = db.create_edge(a, d, "KNOWS", vec![]).unwrap();

    db.delete_edge(e1).unwrap();

    let neighbors = db.get_out_neighbors(a).unwrap();
    assert_eq!(neighbors, vec![d, b]);

    cleanup_dir(&dir);
}

#[test]
fn delete_edge_unlinks_middle_from_in_chain() {
    let dir = temp_dir("delete_edge_unlinks_middle_from_in_chain");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let c = db.create_node("C", vec![]).unwrap();
    let x = db.create_node("X", vec![]).unwrap();

    let _e0 = db.create_edge(a, x, "KNOWS", vec![]).unwrap();
    let e1 = db.create_edge(b, x, "KNOWS", vec![]).unwrap();
    let _e2 = db.create_edge(c, x, "KNOWS", vec![]).unwrap();

    db.delete_edge(e1).unwrap();

    let neighbors = db.get_in_neighbors(x).unwrap();
    assert_eq!(neighbors, vec![c, a]);

    cleanup_dir(&dir);
}

#[test]
fn delete_edge_unlinks_tail_from_out_chain() {
    let dir = temp_dir("delete_edge_unlinks_tail_from_out_chain");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let c = db.create_node("C", vec![]).unwrap();
    let d = db.create_node("D", vec![]).unwrap();

    let e0 = db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    let _e1 = db.create_edge(a, c, "KNOWS", vec![]).unwrap();
    let _e2 = db.create_edge(a, d, "KNOWS", vec![]).unwrap();

    db.delete_edge(e0).unwrap();

    let neighbors = db.get_out_neighbors(a).unwrap();
    assert_eq!(neighbors, vec![d, c]);

    cleanup_dir(&dir);
}
