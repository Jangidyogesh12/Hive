use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::value::Value;
use std::collections::HashMap;

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
    db.set_node_property(node, "score", &Value::Float(3.15))
        .unwrap();

    let val = db.get_node_property(node, "score").unwrap();
    assert_eq!(val, Value::Float(3.15));

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

#[test]
fn node_property_entry_stores_property_key_id() {
    let dir = temp_dir("node_property_key_id");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    db.set_node_property(node, "name", &Value::String("Alice".into()))
        .unwrap();

    let key_id = db.find_property_key("name").unwrap().unwrap();
    let record = db.get_node(node).unwrap();
    assert_eq!(record.properties[0].key_id, key_id);
    assert_eq!(
        db.get_node_property(node, "name").unwrap(),
        Value::String("Alice".into())
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn edge_property_entry_stores_property_key_id() {
    let dir = temp_dir("edge_property_key_id");
    let mut db = HiveDb::open(&dir).unwrap();

    let src = db.create_node().unwrap();
    let dst = db.create_node().unwrap();
    let edge = db.create_edge(src, dst).unwrap();
    db.set_edge_property(edge, "since", &Value::Integer(2024))
        .unwrap();

    let key_id = db.find_property_key("since").unwrap().unwrap();
    let record = db.get_edge(edge).unwrap();
    assert_eq!(record.properties[0].key_id, key_id);
    assert_eq!(
        db.get_edge_property(edge, "since").unwrap(),
        Value::Integer(2024)
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn register_and_get_property_key() {
    let dir = temp_dir("property_key_register");
    let mut db = HiveDb::open(&dir).unwrap();

    let name_id = db.register_property_key("name").unwrap();
    assert_eq!(name_id, 1);

    let age_id = db.register_property_key("age").unwrap();
    assert_eq!(age_id, 2);

    assert_eq!(
        db.get_property_key_name(name_id).unwrap(),
        Some("name".into())
    );
    assert_eq!(
        db.get_property_key_name(age_id).unwrap(),
        Some("age".into())
    );
    assert_eq!(db.find_property_key("name").unwrap(), Some(name_id));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn register_same_property_key_returns_existing_id() {
    let dir = temp_dir("property_key_dedup");
    let mut db = HiveDb::open(&dir).unwrap();

    let id1 = db.register_property_key("name").unwrap();
    let id2 = db.register_property_key("name").unwrap();
    assert_eq!(id1, id2);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn property_key_registration_rolls_back() {
    let dir = temp_dir("property_key_rollback");
    let mut db = HiveDb::open(&dir).unwrap();

    let key_id;
    {
        let mut tx = db.begin().unwrap();
        key_id = tx.register_property_key("rolled_back").unwrap();
        tx.rollback().unwrap();
    }

    assert_eq!(db.get_property_key_name(key_id).unwrap(), None);
    assert_eq!(db.find_property_key("rolled_back").unwrap(), None);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn node_property_registers_key_transactionally() {
    let dir = temp_dir("node_property_key_transactional");
    let mut db = HiveDb::open(&dir).unwrap();
    let node = db.create_node().unwrap();

    {
        let mut tx = db.begin().unwrap();
        tx.set_node_property(node, "rolled_back", &Value::Integer(1))
            .unwrap();
        tx.rollback().unwrap();
    }
    assert_eq!(db.find_property_key("rolled_back").unwrap(), None);

    db.set_node_property(node, "committed", &Value::Integer(2))
        .unwrap();
    let key_id = db.find_property_key("committed").unwrap().unwrap();
    assert_eq!(
        db.get_property_key_name(key_id).unwrap(),
        Some("committed".into())
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn edge_property_registers_key() {
    let dir = temp_dir("edge_property_key_register");
    let mut db = HiveDb::open(&dir).unwrap();
    let src = db.create_node().unwrap();
    let dst = db.create_node().unwrap();
    let edge = db.create_edge(src, dst).unwrap();

    db.set_edge_property(edge, "since", &Value::Integer(2024))
        .unwrap();

    let key_id = db.find_property_key("since").unwrap().unwrap();
    assert_eq!(
        db.get_property_key_name(key_id).unwrap(),
        Some("since".into())
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn property_keys_persist_after_reopen() {
    let dir = temp_dir("property_key_persist");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let node = db.create_node().unwrap();
        db.set_node_property(node, "name", &Value::String("Alice".into()))
            .unwrap();
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let key_id = db.find_property_key("name").unwrap().unwrap();
        assert_eq!(
            db.get_property_key_name(key_id).unwrap(),
            Some("name".into())
        );
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn committed_property_key_survives_wal_recovery() {
    let dir = temp_dir("property_key_wal_recovery");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let key_id = db.register_property_key("recovered").unwrap();
        assert_eq!(key_id, 1);
        drop(db);
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        assert_eq!(
            db.get_property_key_name(1).unwrap(),
            Some("recovered".into())
        );
        db.close();
    }

    cleanup_dir(&dir);
}

// ── Step 4: Property Name Introspection and Whole-Entity Return ──

#[test]
fn list_node_properties_returns_names_and_values() {
    let dir = temp_dir("list_node_props");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    db.set_node_property(node, "name", &Value::String("Alice".into()))
        .unwrap();
    db.set_node_property(node, "age", &Value::Integer(30))
        .unwrap();

    let mut props = db.list_node_properties(node).unwrap();
    props.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(props.len(), 2);
    assert_eq!(props[0].0, "age");
    assert_eq!(props[0].1, Value::Integer(30));
    assert_eq!(props[1].0, "name");
    assert_eq!(props[1].1, Value::String("Alice".into()));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn list_edge_properties_returns_names_and_values() {
    let dir = temp_dir("list_edge_props");
    let mut db = HiveDb::open(&dir).unwrap();

    let src = db.create_node().unwrap();
    let dst = db.create_node().unwrap();
    let edge = db.create_edge(src, dst).unwrap();
    db.set_edge_property(edge, "since", &Value::Integer(2024))
        .unwrap();
    db.set_edge_property(edge, "weight", &Value::Float(1.5))
        .unwrap();

    let mut props = db.list_edge_properties(edge).unwrap();
    props.sort_by(|a, b| a.0.cmp(&b.0));

    assert_eq!(props.len(), 2);
    assert_eq!(props[0].0, "since");
    assert_eq!(props[0].1, Value::Integer(2024));
    assert_eq!(props[1].0, "weight");
    assert_eq!(props[1].1, Value::Float(1.5));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn list_node_properties_empty_when_no_properties() {
    let dir = temp_dir("list_node_props_empty");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    let props = db.list_node_properties(node).unwrap();
    assert!(props.is_empty());

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn list_node_properties_includes_long_strings() {
    let dir = temp_dir("list_node_props_long");
    let mut db = HiveDb::open(&dir).unwrap();

    let node = db.create_node().unwrap();
    let long = "x".repeat(100);
    db.set_node_property(node, "bio", &Value::String(long.clone()))
        .unwrap();

    let props = db.list_node_properties(node).unwrap();
    assert_eq!(props.len(), 1);
    assert_eq!(props[0].0, "bio");
    assert_eq!(props[0].1, Value::String(long));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn query_return_node_produces_entity_map() {
    let dir = temp_dir("return_node_entity");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:Person {name: "Alice", age: 30})"#)
        .unwrap();

    let result = db.execute("MATCH (n:Person) RETURN n").unwrap();

    assert_eq!(result.columns, vec!["n"]);
    assert_eq!(result.rows.len(), 1);

    match &result.rows[0][0] {
        Value::Map(map) => {
            assert_eq!(map.get("id"), Some(&Value::Integer(1)));
            assert_eq!(map.get("label"), Some(&Value::String("Person".into())));
            match map.get("properties") {
                Some(Value::Map(props)) => {
                    assert_eq!(props.get("name"), Some(&Value::String("Alice".into())));
                    assert_eq!(props.get("age"), Some(&Value::Integer(30)));
                }
                other => panic!("expected properties map, got {:?}", other),
            }
        }
        other => panic!("expected entity map, got {:?}", other),
    }

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn query_return_edge_produces_entity_map() {
    let dir = temp_dir("return_edge_entity");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(
        r#"CREATE (a:Person {name: "Alice"})-[:KNOWS {since: 2020}]->(b:Person {name: "Bob"})"#,
    )
    .unwrap();

    let result = db.execute("MATCH (a)-[r:KNOWS]->(b) RETURN r").unwrap();

    assert_eq!(result.columns, vec!["r"]);
    assert_eq!(result.rows.len(), 1);

    match &result.rows[0][0] {
        Value::Map(map) => {
            assert_eq!(map.get("type"), Some(&Value::String("KNOWS".into())));
            assert!(matches!(map.get("id"), Some(Value::Integer(_))));
            assert!(matches!(map.get("src"), Some(Value::Integer(_))));
            assert!(matches!(map.get("dst"), Some(Value::Integer(_))));
            assert_ne!(map.get("src"), map.get("dst"));
            match map.get("properties") {
                Some(Value::Map(props)) => {
                    assert_eq!(props.get("since"), Some(&Value::Integer(2020)));
                }
                other => panic!("expected properties map, got {:?}", other),
            }
        }
        other => panic!("expected entity map, got {:?}", other),
    }

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn value_display_map() {
    let mut map = HashMap::new();
    map.insert("name".to_string(), Value::String("Alice".into()));
    map.insert("age".to_string(), Value::Integer(30));
    let v = Value::Map(map);
    let s = v.to_string();
    assert!(s.contains("name: Alice"));
    assert!(s.contains("age: 30"));
}

#[test]
fn value_display_list() {
    let v = Value::List(vec![
        Value::Integer(1),
        Value::String("two".into()),
        Value::Null,
    ]);
    assert_eq!(v.to_string(), "[1, two, NULL]");
}
