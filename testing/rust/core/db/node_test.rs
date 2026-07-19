use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;

#[test]
fn create_node_returns_valid_id() {
    let dir = temp_dir("create_node_valid_id");
    let mut db = HiveDb::open(&dir).unwrap();

    let node_id = db.create_node().unwrap();
    assert!(node_id != u64::MAX);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_and_read_node() {
    let dir = temp_dir("create_and_read_node");
    let mut db = HiveDb::open(&dir).unwrap();

    let node_id = db.create_node().unwrap();
    let node = db.get_node(node_id).unwrap();

    assert_eq!(node.id, 1);
    assert_eq!(node.label_id, 0);
    assert_eq!(node.first_out_edge, u64::MAX);
    assert_eq!(node.first_in_edge, u64::MAX);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_multiple_nodes_returns_distinct_ids() {
    let dir = temp_dir("create_multiple_nodes");
    let mut db = HiveDb::open(&dir).unwrap();

    let id1 = db.create_node().unwrap();
    let id2 = db.create_node().unwrap();
    let id3 = db.create_node().unwrap();

    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);

    let n1 = db.get_node(id1).unwrap();
    let n2 = db.get_node(id2).unwrap();
    let n3 = db.get_node(id3).unwrap();

    assert_eq!(n1.id, 1);
    assert_eq!(n2.id, 2);
    assert_eq!(n3.id, 3);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_node_persists_after_reopen() {
    let dir = temp_dir("create_node_persists");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let _ = db.create_node().unwrap();
        let id2 = db.create_node().unwrap();

        let node = db.get_node(id2).unwrap();
        assert_eq!(node.id, 2);
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let node = db.get_node(crate::types::pack_record_id(1, 1)).unwrap();
        assert_eq!(node.id, 2);
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn get_node_returns_error_for_invalid_id() {
    let dir = temp_dir("get_node_invalid");
    let mut db = HiveDb::open(&dir).unwrap();

    let result = db.get_node(u64::MAX);
    assert!(result.is_err());

    db.close();
    cleanup_dir(&dir);
}
