use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::value::Value;

#[test]
fn set_and_get_long_string_node_property() {
    let dir = temp_dir("long_string_node");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    let long_value = "a".repeat(100);
    db.set_node_property(node, "bio", &Value::String(long_value.clone()))
        .unwrap();

    let val = db.get_node_property(node, "bio").unwrap();
    assert_eq!(val, Value::String(long_value));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_and_get_long_string_edge_property() {
    let dir = temp_dir("long_string_edge");
    let mut db = HiveDb::open(&dir).unwrap();

    let alice = db.create_node().unwrap();
    let bob = db.create_node().unwrap();
    let edge = db.create_edge(alice, bob).unwrap();

    let long_value = "hello world! ".repeat(10);
    db.set_edge_property(edge, "description", &Value::String(long_value.clone()))
        .unwrap();

    let val = db.get_edge_property(edge, "description").unwrap();
    assert_eq!(val, Value::String(long_value));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn short_string_stays_inline() {
    let dir = temp_dir("short_inline");
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
fn exactly_15_bytes_stays_inline() {
    let dir = temp_dir("exactly_15");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    let val_15 = "x".repeat(15);
    db.set_node_property(node, "key", &Value::String(val_15.clone()))
        .unwrap();

    let val = db.get_node_property(node, "key").unwrap();
    assert_eq!(val, Value::String(val_15));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn sixteen_bytes_goes_to_overflow() {
    let dir = temp_dir("16_overflow");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    let val_16 = "y".repeat(16);
    db.set_node_property(node, "key", &Value::String(val_16.clone()))
        .unwrap();

    let val = db.get_node_property(node, "key").unwrap();
    assert_eq!(val, Value::String(val_16));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn very_long_string_1000_bytes() {
    let dir = temp_dir("very_long");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    let long_value = "z".repeat(1000);
    db.set_node_property(node, "data", &Value::String(long_value.clone()))
        .unwrap();

    let val = db.get_node_property(node, "data").unwrap();
    assert_eq!(val, Value::String(long_value));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn long_string_persists_after_reopen() {
    let dir = temp_dir("long_persist");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let node = db.create_node().unwrap();
        let long_value = "persist me! ".repeat(20);
        db.set_node_property(node, "text", &Value::String(long_value))
            .unwrap();
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let node = crate::types::pack_record_id(1, 0);
        let expected = "persist me! ".repeat(20);
        let val = db.get_node_property(node, "text").unwrap();
        assert_eq!(val, Value::String(expected));
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn overwrite_short_with_long_string() {
    let dir = temp_dir("overwrite_short_long");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    db.set_node_property(node, "key", &Value::String("short".into()))
        .unwrap();
    let long_value = "x".repeat(200);
    db.set_node_property(node, "key", &Value::String(long_value.clone()))
        .unwrap();

    let val = db.get_node_property(node, "key").unwrap();
    assert_eq!(val, Value::String(long_value));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn overwrite_long_with_short_string() {
    let dir = temp_dir("overwrite_long_short");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    let long_value = "y".repeat(200);
    db.set_node_property(node, "key", &Value::String(long_value))
        .unwrap();
    db.set_node_property(node, "key", &Value::String("short".into()))
        .unwrap();

    let val = db.get_node_property(node, "key").unwrap();
    assert_eq!(val, Value::String("short".into()));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn multiple_long_strings() {
    let dir = temp_dir("multiple_long");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    let val_a = "aaa".repeat(50);
    let val_b = "bbb".repeat(50);
    let val_c = "ccc".repeat(50);

    db.set_node_property(node, "a", &Value::String(val_a.clone()))
        .unwrap();
    db.set_node_property(node, "b", &Value::String(val_b.clone()))
        .unwrap();
    db.set_node_property(node, "c", &Value::String(val_c.clone()))
        .unwrap();

    assert_eq!(
        db.get_node_property(node, "a").unwrap(),
        Value::String(val_a)
    );
    assert_eq!(
        db.get_node_property(node, "b").unwrap(),
        Value::String(val_b)
    );
    assert_eq!(
        db.get_node_property(node, "c").unwrap(),
        Value::String(val_c)
    );

    db.close();
    cleanup_dir(&dir);
}
