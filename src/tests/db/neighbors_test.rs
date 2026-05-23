use super::super::utils::utils::{cleanup_dir, temp_dir};

#[test]
fn get_out_neighbors_returns_empty_for_node_with_no_edges() {
    let dir = temp_dir("get_out_neighbors_returns_empty_for_node_with_no_edges");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let id = db.create_node("Person", vec![]).unwrap();
    let neighbors = db.get_out_neighbors(id).unwrap();

    assert!(neighbors.is_empty());

    cleanup_dir(&dir);
}

#[test]
fn get_out_neighbors_returns_single_dst() {
    let dir = temp_dir("get_out_neighbors_returns_single_dst");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    db.create_edge(a, b, "KNOWS", vec![]).unwrap();

    let neighbors = db.get_out_neighbors(a).unwrap();
    assert_eq!(neighbors, vec![b]);

    cleanup_dir(&dir);
}

#[test]
fn get_out_neighbors_returns_multiple_dsts() {
    let dir = temp_dir("get_out_neighbors_returns_multiple_dsts");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let c = db.create_node("C", vec![]).unwrap();
    let d = db.create_node("D", vec![]).unwrap();

    db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    db.create_edge(a, c, "KNOWS", vec![]).unwrap();
    db.create_edge(a, d, "KNOWS", vec![]).unwrap();

    let neighbors = db.get_out_neighbors(a).unwrap();
    assert_eq!(neighbors, vec![d, c, b]);

    cleanup_dir(&dir);
}

#[test]
fn get_out_neighbors_skips_deleted_edges() {
    let dir = temp_dir("get_out_neighbors_skips_deleted_edges");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let c = db.create_node("C", vec![]).unwrap();

    let e0 = db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    db.create_edge(a, c, "KNOWS", vec![]).unwrap();

    db.delete_edge(e0).unwrap();

    let neighbors = db.get_out_neighbors(a).unwrap();
    assert_eq!(neighbors, vec![c]);

    cleanup_dir(&dir);
}

#[test]
fn get_out_neighbors_skips_deleted_dst_nodes() {
    let dir = temp_dir("get_out_neighbors_skips_deleted_dst_nodes");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let c = db.create_node("C", vec![]).unwrap();

    db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    db.create_edge(a, c, "KNOWS", vec![]).unwrap();

    db.delete_node(b).unwrap();

    let neighbors = db.get_out_neighbors(a).unwrap();
    assert_eq!(neighbors, vec![c]);

    cleanup_dir(&dir);
}

#[test]
fn get_in_neighbors_returns_empty_for_node_with_no_edges() {
    let dir = temp_dir("get_in_neighbors_returns_empty_for_node_with_no_edges");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let id = db.create_node("Person", vec![]).unwrap();
    let neighbors = db.get_in_neighbors(id).unwrap();

    assert!(neighbors.is_empty());

    cleanup_dir(&dir);
}

#[test]
fn get_in_neighbors_returns_single_src() {
    let dir = temp_dir("get_in_neighbors_returns_single_src");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    db.create_edge(a, b, "KNOWS", vec![]).unwrap();

    let neighbors = db.get_in_neighbors(b).unwrap();
    assert_eq!(neighbors, vec![a]);

    cleanup_dir(&dir);
}

#[test]
fn get_in_neighbors_returns_multiple_srcs() {
    let dir = temp_dir("get_in_neighbors_returns_multiple_srcs");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let c = db.create_node("C", vec![]).unwrap();
    let x = db.create_node("X", vec![]).unwrap();

    db.create_edge(a, x, "KNOWS", vec![]).unwrap();
    db.create_edge(b, x, "KNOWS", vec![]).unwrap();
    db.create_edge(c, x, "KNOWS", vec![]).unwrap();

    let neighbors = db.get_in_neighbors(x).unwrap();
    assert_eq!(neighbors, vec![c, b, a]);

    cleanup_dir(&dir);
}

#[test]
fn get_in_neighbors_skips_deleted_edges() {
    let dir = temp_dir("get_in_neighbors_skips_deleted_edges");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let x = db.create_node("X", vec![]).unwrap();

    let e0 = db.create_edge(b, x, "KNOWS", vec![]).unwrap();
    db.create_edge(a, x, "KNOWS", vec![]).unwrap();

    db.delete_edge(e0).unwrap();

    let neighbors = db.get_in_neighbors(x).unwrap();
    assert_eq!(neighbors, vec![a]);

    cleanup_dir(&dir);
}

#[test]
fn get_in_neighbors_skips_deleted_src_nodes() {
    let dir = temp_dir("get_in_neighbors_skips_deleted_src_nodes");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let x = db.create_node("X", vec![]).unwrap();

    db.create_edge(a, x, "KNOWS", vec![]).unwrap();
    db.create_edge(b, x, "KNOWS", vec![]).unwrap();

    db.delete_node(b).unwrap();

    let neighbors = db.get_in_neighbors(x).unwrap();
    assert_eq!(neighbors, vec![a]);

    cleanup_dir(&dir);
}
