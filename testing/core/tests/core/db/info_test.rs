use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::value::Value;

#[test]
fn info_reports_empty_database_stats() {
    let dir = temp_dir("info_reports_empty_database_stats");
    let mut db = HiveDb::open(&dir).unwrap();

    let info = db.info().unwrap();

    assert_eq!(info.version, 1);
    assert_eq!(info.live_node_count, 0);
    assert_eq!(info.live_edge_count, 0);
    assert_eq!(info.property_count, 0);
    assert_eq!(info.node_record_count, 0);
    assert_eq!(info.edge_record_count, 0);
    assert_eq!(info.property_record_count, 0);
    assert_eq!(info.free_node_count, 0);
    assert_eq!(info.free_edge_count, 0);

    cleanup_dir(&dir);
}

#[test]
fn info_distinguishes_live_counts_from_physical_records() {
    let dir = temp_dir("info_distinguishes_live_counts_from_physical_records");
    let mut db = HiveDb::open(&dir).unwrap();

    let alice = db.create_node("Person", vec![]).unwrap();
    let bob = db.create_node("Person", vec![]).unwrap();
    db.set_node_property(alice, "name", Value::String("Alice".to_string()))
        .unwrap();
    let edge = db.create_edge(alice, bob, "KNOWS", vec![]).unwrap();

    db.delete_edge(edge).unwrap();
    db.delete_node(bob).unwrap();

    let info = db.info().unwrap();

    assert_eq!(info.live_node_count, 1);
    assert_eq!(info.live_edge_count, 0);
    assert_eq!(info.property_count, 1);
    assert_eq!(info.node_record_count, 2);
    assert_eq!(info.edge_record_count, 1);
    assert_eq!(info.property_record_count, 1);
    assert_eq!(info.free_node_count, 1);
    assert_eq!(info.free_edge_count, 1);

    cleanup_dir(&dir);
}
