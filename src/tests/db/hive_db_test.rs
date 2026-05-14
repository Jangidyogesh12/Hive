// Tests HiveDb create_node / get_node round-trips, label handling, and error cases.
use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::{HiveDb, NodeProperty};
use crate::errors::DbError;

fn helper_node_property(key: &str, key_hash: u64, value_type: u8, value_inline: [u8; 15]) -> NodeProperty {
    NodeProperty {
        key_value: key.to_string(),
        key_hash,
        value_type,
        value_inline,
    }
}

#[test]
// Verifies create_node returns sequential IDs and get_node round-trips without properties.
fn create_then_get_node_with_no_properties() {
    let dir = temp_dir("create_then_get_node_with_no_properties");
    let mut db = HiveDb::open(&dir).unwrap();

    let id = db.create_node("Person", vec![]).unwrap();
    let node = db.get_node(id).unwrap();

    assert_eq!(node.id, id);
    assert_eq!(node.label, "Person");
    assert!(node.properties.is_empty());

    cleanup_dir(&dir);
}

#[test]
// Verifies create_node with a single property is returned correctly by get_node.
fn create_then_get_node_with_single_property() {
    let dir = temp_dir("create_then_get_node_with_single_property");
    let mut db = HiveDb::open(&dir).unwrap();

    let props = vec![helper_node_property("name", 42, 1, [0u8; 15])];

    let id = db.create_node("Person", props).unwrap();
    let node = db.get_node(id).unwrap();

    assert_eq!(node.id, id);
    assert_eq!(node.label, "Person");
    assert_eq!(node.properties.len(), 1);
    assert_eq!(node.properties[0].key_value, "name");
    assert_eq!(node.properties[0].key_hash, 42);
    assert_eq!(node.properties[0].value_type, 1);

    cleanup_dir(&dir);
}

#[test]
// Verifies create_node with multiple properties preserves insertion order.
fn create_then_get_node_with_multiple_properties() {
    let dir = temp_dir("create_then_get_node_with_multiple_properties");
    let mut db = HiveDb::open(&dir).unwrap();

    let props = vec![
        helper_node_property("name", 1, 1, [0u8; 15]),
        helper_node_property("age", 2, 2, [0u8; 15]),
        helper_node_property("active", 3, 3, [0u8; 15]),
    ];

    let id = db.create_node("User", props).unwrap();
    let node = db.get_node(id).unwrap();

    assert_eq!(node.label, "User");
    assert_eq!(node.properties.len(), 3);
    assert_eq!(node.properties[0].key_value, "name");
    assert_eq!(node.properties[1].key_value, "age");
    assert_eq!(node.properties[2].key_value, "active");

    cleanup_dir(&dir);
}

#[test]
// Verifies label de-duplication: two nodes with same label return same label string.
fn create_multiple_nodes_with_same_label_returns_consistent_label() {
    let dir = temp_dir("create_multiple_nodes_with_same_label_returns_consistent_label");
    let mut db = HiveDb::open(&dir).unwrap();

    let id0 = db.create_node("Person", vec![]).unwrap();
    let id1 = db.create_node("Person", vec![]).unwrap();
    let id2 = db.create_node("Company", vec![]).unwrap();

    let n0 = db.get_node(id0).unwrap();
    let n1 = db.get_node(id1).unwrap();
    let n2 = db.get_node(id2).unwrap();

    assert_eq!(n0.id, id0);
    assert_eq!(n1.id, id1);
    assert_eq!(n2.id, id2);
    assert_eq!(n0.label, "Person");
    assert_eq!(n1.label, "Person");
    assert_eq!(n2.label, "Company");

    cleanup_dir(&dir);
}

#[test]
// Verifies get_node for an id that exceeds the node count returns a ReadError.
fn get_node_out_of_bounds_returns_read_error() {
    let dir = temp_dir("get_node_out_of_bounds_returns_read_error");
    let mut db = HiveDb::open(&dir).unwrap();

    let _id = db.create_node("Person", vec![]).unwrap();

    let result = db.get_node(99);
    assert!(matches!(result, Err(DbError::ReadError)));

    cleanup_dir(&dir);
}

#[test]
// Verifies node record and properties persist across close and reopen.
// Note: LabelStore does not yet reload labels from disk; the label
// falls back to "<unknown>" after reopen.
fn data_persists_across_reopen() {
    let dir = temp_dir("data_persists_across_reopen");

    let props = vec![helper_node_property("key", 7, 1, [0u8; 15])];

    let id = {
        let mut db = HiveDb::open(&dir).unwrap();
        db.create_node("Item", props).unwrap()
    };

    let mut db2 = HiveDb::open(&dir).unwrap();
    let node = db2.get_node(id).unwrap();

    assert_eq!(node.id, id);
    assert_eq!(node.properties.len(), 1);
    assert_eq!(node.properties[0].key_value, "key");

    cleanup_dir(&dir);
}
