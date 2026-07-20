use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::value::Value;

#[test]
fn set_and_get_integer_property() {
    let dir = temp_dir("prop_integer");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    db.set_node_property(node, "age", &Value::Integer(42))
        .unwrap();

    let val = db.get_node_property(node, "age").unwrap();
    assert_eq!(val, Value::Integer(42));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_and_get_float_property() {
    let dir = temp_dir("prop_float");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    db.set_node_property(node, "score", &Value::Float(3.14))
        .unwrap();

    let val = db.get_node_property(node, "score").unwrap();
    assert_eq!(val, Value::Float(3.14));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_and_get_boolean_property() {
    let dir = temp_dir("prop_boolean");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    db.set_node_property(node, "active", &Value::Boolean(true))
        .unwrap();

    let val = db.get_node_property(node, "active").unwrap();
    assert_eq!(val, Value::Boolean(true));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_and_get_string_property() {
    let dir = temp_dir("prop_string");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    db.set_node_property(node, "name", &Value::String("Alice".into()))
        .unwrap();

    let val = db.get_node_property(node, "name").unwrap();
    assert_eq!(val, Value::String("Alice".into()));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_and_get_null_property() {
    let dir = temp_dir("prop_null");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    db.set_node_property(node, "deleted", &Value::Null).unwrap();

    let val = db.get_node_property(node, "deleted").unwrap();
    assert_eq!(val, Value::Null);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_multiple_properties() {
    let dir = temp_dir("prop_multiple");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    db.set_node_property(node, "name", &Value::String("Bob".into()))
        .unwrap();
    db.set_node_property(node, "age", &Value::Integer(25))
        .unwrap();
    db.set_node_property(node, "active", &Value::Boolean(true))
        .unwrap();

    assert_eq!(
        db.get_node_property(node, "name").unwrap(),
        Value::String("Bob".into())
    );
    assert_eq!(
        db.get_node_property(node, "age").unwrap(),
        Value::Integer(25)
    );
    assert_eq!(
        db.get_node_property(node, "active").unwrap(),
        Value::Boolean(true)
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn update_existing_property() {
    let dir = temp_dir("prop_update");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    db.set_node_property(node, "score", &Value::Integer(10))
        .unwrap();
    db.set_node_property(node, "score", &Value::Integer(99))
        .unwrap();

    let val = db.get_node_property(node, "score").unwrap();
    assert_eq!(val, Value::Integer(99));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn get_nonexistent_property_returns_error() {
    let dir = temp_dir("prop_nonexistent");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    let result = db.get_node_property(node, "missing");
    assert!(result.is_err());

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn properties_persist_after_reopen() {
    let dir = temp_dir("prop_persist");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let node = db.create_node().unwrap();
        db.set_node_property(node, "name", &Value::String("Charlie".into()))
            .unwrap();
        db.set_node_property(node, "age", &Value::Integer(30))
            .unwrap();
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let node = crate::types::pack_record_id(1, 0);
        assert_eq!(
            db.get_node_property(node, "name").unwrap(),
            Value::String("Charlie".into())
        );
        assert_eq!(
            db.get_node_property(node, "age").unwrap(),
            Value::Integer(30)
        );
        db.close();
    }

    cleanup_dir(&dir);
}
