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

    assert_eq!(people, vec![0]);
    assert_eq!(by_name, vec![0]);
    assert_eq!(edges, vec![0]);

    cleanup_dir(&dir);
}

#[test]
fn deleting_node_removes_it_from_indexes() {
    let dir = temp_dir("deleting_node_removes_it_from_indexes");
    let mut db = HiveDb::open(&dir).unwrap();

    let node_id = db.create_node("Person", vec![]).unwrap();
    db.set_node_property(node_id, "name", Value::String("Alice".to_string()))
        .unwrap();

    assert_eq!(db.lookup_node_ids_by_label("Person").unwrap(), vec![node_id]);
    assert_eq!(
        db.lookup_node_ids_by_property("name", &Value::String("Alice".to_string()))
            .unwrap(),
        vec![node_id]
    );

    db.delete_node(node_id).unwrap();

    assert!(db.lookup_node_ids_by_label("Person").unwrap().is_empty());
    assert!(db
        .lookup_node_ids_by_property("name", &Value::String("Alice".to_string()))
        .unwrap()
        .is_empty());

    cleanup_dir(&dir);
}

#[test]
fn updating_node_property_moves_index_entry() {
    let dir = temp_dir("updating_node_property_moves_index_entry");
    let mut db = HiveDb::open(&dir).unwrap();

    let node_id = db.create_node("Person", vec![]).unwrap();
    db.set_node_property(node_id, "age", Value::Integer(25)).unwrap();
    db.set_node_property(node_id, "age", Value::Integer(26)).unwrap();

    assert!(db
        .lookup_node_ids_by_property("age", &Value::Integer(25))
        .unwrap()
        .is_empty());
    assert_eq!(
        db.lookup_node_ids_by_property("age", &Value::Integer(26))
            .unwrap(),
        vec![node_id]
    );

    cleanup_dir(&dir);
}
