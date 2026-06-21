use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::value::Value;

#[test]
fn index_file_is_created_and_loaded_on_reopen() {
    let dir = temp_dir("index_file_is_created_and_loaded_on_reopen");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        db.create_node("Person", vec![]).unwrap();
        db.set_node_property(0, "name", Value::String("Alice".to_string()))
            .unwrap();
        db.create_edge(0, 0, "SELF", vec![]).unwrap();
    }

    assert!(dir.join("indexes.hive").exists());

    let reopened = HiveDb::open(&dir).unwrap();
    let people = reopened.lookup_node_ids_by_label("Person").unwrap();
    let by_name = reopened
        .lookup_node_ids_by_property("name", &Value::String("Alice".to_string()))
        .unwrap();
    let edges = reopened.lookup_edge_ids_by_type("SELF").unwrap();
    let by_edge_property = reopened
        .lookup_edge_ids_by_property("kind", &Value::String("loop".to_string()))
        .unwrap();

    assert_eq!(people, vec![0]);
    assert_eq!(by_name, vec![0]);
    assert_eq!(edges, vec![0]);
    assert!(by_edge_property.is_empty());

    cleanup_dir(&dir);
}

#[test]
fn edge_property_index_is_created_and_loaded_on_reopen() {
    let dir = temp_dir("edge_property_index_is_created_and_loaded_on_reopen");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let a = db.create_node("Person", vec![]).unwrap();
        let b = db.create_node("Person", vec![]).unwrap();
        let edge_id = db.create_edge(a, b, "KNOWS", vec![]).unwrap();
        db.set_edge_property(edge_id, "since", Value::Integer(2020))
            .unwrap();
    }

    let reopened = HiveDb::open(&dir).unwrap();
    assert_eq!(
        reopened
            .lookup_edge_ids_by_property("since", &Value::Integer(2020))
            .unwrap(),
        vec![0]
    );

    cleanup_dir(&dir);
}

#[test]
fn updating_edge_property_moves_index_entry() {
    let dir = temp_dir("updating_edge_property_moves_index_entry");
    let mut db = HiveDb::open(&dir).unwrap();

    let a = db.create_node("Person", vec![]).unwrap();
    let b = db.create_node("Person", vec![]).unwrap();
    let edge_id = db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    db.set_edge_property(edge_id, "since", Value::Integer(2020))
        .unwrap();
    db.set_edge_property(edge_id, "since", Value::Integer(2024))
        .unwrap();

    assert!(
        db.lookup_edge_ids_by_property("since", &Value::Integer(2020))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.lookup_edge_ids_by_property("since", &Value::Integer(2024))
            .unwrap(),
        vec![edge_id]
    );

    cleanup_dir(&dir);
}

#[test]
fn deleting_edge_removes_it_from_edge_property_index() {
    let dir = temp_dir("deleting_edge_removes_it_from_edge_property_index");
    let mut db = HiveDb::open(&dir).unwrap();

    let a = db.create_node("Person", vec![]).unwrap();
    let b = db.create_node("Person", vec![]).unwrap();
    let edge_id = db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    db.set_edge_property(edge_id, "since", Value::Integer(2020))
        .unwrap();

    assert_eq!(
        db.lookup_edge_ids_by_property("since", &Value::Integer(2020))
            .unwrap(),
        vec![edge_id]
    );

    db.delete_edge(edge_id).unwrap();

    assert!(
        db.lookup_edge_ids_by_property("since", &Value::Integer(2020))
            .unwrap()
            .is_empty()
    );

    cleanup_dir(&dir);
}

#[test]
fn deleting_node_removes_it_from_indexes() {
    let dir = temp_dir("deleting_node_removes_it_from_indexes");
    let mut db = HiveDb::open(&dir).unwrap();

    let node_id = db.create_node("Person", vec![]).unwrap();
    db.set_node_property(node_id, "name", Value::String("Alice".to_string()))
        .unwrap();

    assert_eq!(
        db.lookup_node_ids_by_label("Person").unwrap(),
        vec![node_id]
    );
    assert_eq!(
        db.lookup_node_ids_by_property("name", &Value::String("Alice".to_string()))
            .unwrap(),
        vec![node_id]
    );

    db.delete_node(node_id).unwrap();

    assert!(db.lookup_node_ids_by_label("Person").unwrap().is_empty());
    assert!(
        db.lookup_node_ids_by_property("name", &Value::String("Alice".to_string()))
            .unwrap()
            .is_empty()
    );

    cleanup_dir(&dir);
}

#[test]
fn updating_node_property_moves_index_entry() {
    let dir = temp_dir("updating_node_property_moves_index_entry");
    let mut db = HiveDb::open(&dir).unwrap();

    let node_id = db.create_node("Person", vec![]).unwrap();
    db.set_node_property(node_id, "age", Value::Integer(25))
        .unwrap();
    db.set_node_property(node_id, "age", Value::Integer(26))
        .unwrap();

    assert!(
        db.lookup_node_ids_by_property("age", &Value::Integer(25))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.lookup_node_ids_by_property("age", &Value::Integer(26))
            .unwrap(),
        vec![node_id]
    );

    cleanup_dir(&dir);
}
