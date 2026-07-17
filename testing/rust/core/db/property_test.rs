use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::value::Value;

#[test]
fn set_and_get_node_integer_property() {
    let dir = temp_dir("set_and_get_node_integer_property");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let node_id = db.create_node("Person", vec![]).unwrap();
    db.set_node_property(node_id, "age", Value::Integer(30))
        .unwrap();

    let val = db.get_node_property(node_id, "age").unwrap();
    assert_eq!(val, Some(Value::Integer(30)));

    cleanup_dir(&dir);
}

#[test]
fn set_and_get_node_float_property() {
    let dir = temp_dir("set_and_get_node_float_property");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let node_id = db.create_node("Person", vec![]).unwrap();
    db.set_node_property(node_id, "score", Value::Float(3.14))
        .unwrap();

    let val = db.get_node_property(node_id, "score").unwrap();
    assert_eq!(val, Some(Value::Float(3.14)));

    cleanup_dir(&dir);
}

#[test]
fn set_and_get_node_boolean_property() {
    let dir = temp_dir("set_and_get_node_boolean_property");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let node_id = db.create_node("Person", vec![]).unwrap();
    db.set_node_property(node_id, "active", Value::Boolean(true))
        .unwrap();

    let val = db.get_node_property(node_id, "active").unwrap();
    assert_eq!(val, Some(Value::Boolean(true)));

    cleanup_dir(&dir);
}

#[test]
fn set_and_get_node_string_property() {
    let dir = temp_dir("set_and_get_node_string_property");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let node_id = db.create_node("Person", vec![]).unwrap();
    db.set_node_property(node_id, "name", Value::String("Alice".to_string()))
        .unwrap();

    let val = db.get_node_property(node_id, "name").unwrap();
    assert_eq!(val, Some(Value::String("Alice".to_string())));

    cleanup_dir(&dir);
}

#[test]
fn set_node_property_overwrites_existing() {
    let dir = temp_dir("set_node_property_overwrites_existing");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let node_id = db.create_node("Person", vec![]).unwrap();
    db.set_node_property(node_id, "age", Value::Integer(25))
        .unwrap();
    db.set_node_property(node_id, "age", Value::Integer(26))
        .unwrap();

    let val = db.get_node_property(node_id, "age").unwrap();
    assert_eq!(val, Some(Value::Integer(26)));

    cleanup_dir(&dir);
}

#[test]
fn get_node_property_missing_key_returns_none() {
    let dir = temp_dir("get_node_property_missing_key_returns_none");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let node_id = db.create_node("Person", vec![]).unwrap();

    let val = db.get_node_property(node_id, "nonexistent").unwrap();
    assert_eq!(val, None);

    cleanup_dir(&dir);
}

#[test]
fn set_and_get_edge_property() {
    let dir = temp_dir("set_and_get_edge_property");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let src = db.create_node("A", vec![]).unwrap();
    let dst = db.create_node("B", vec![]).unwrap();
    let edge_id = db.create_edge(src, dst, "KNOWS", vec![]).unwrap();
    db.set_edge_property(edge_id, "since", Value::Integer(2020))
        .unwrap();

    let val = db.get_edge_property(edge_id, "since").unwrap();
    assert_eq!(val, Some(Value::Integer(2020)));

    cleanup_dir(&dir);
}

#[test]
fn set_edge_property_overwrites_existing() {
    let dir = temp_dir("set_edge_property_overwrites_existing");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let src = db.create_node("A", vec![]).unwrap();
    let dst = db.create_node("B", vec![]).unwrap();
    let edge_id = db.create_edge(src, dst, "KNOWS", vec![]).unwrap();
    db.set_edge_property(edge_id, "weight", Value::Float(1.0))
        .unwrap();
    db.set_edge_property(edge_id, "weight", Value::Float(2.5))
        .unwrap();

    let val = db.get_edge_property(edge_id, "weight").unwrap();
    assert_eq!(val, Some(Value::Float(2.5)));

    cleanup_dir(&dir);
}
