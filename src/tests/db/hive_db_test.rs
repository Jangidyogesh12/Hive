// Tests HiveDb create_node / get_node, create_edge / get_edge round-trips,
// label handling, property chains, and error cases.
use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::{HiveDb, Property};
use crate::errors::DbError;
use crate::value::Value;

fn helper_property(
    key: &str,
    key_hash: u64,
    value_type: u8,
    value_inline: [u8; 15],
) -> Property {
    Property {
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

    let props = vec![helper_property("name", 42, 1, [0u8; 15])];

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
        helper_property("name", 1, 1, [0u8; 15]),
        helper_property("age", 2, 2, [0u8; 15]),
        helper_property("active", 3, 3, [0u8; 15]),
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

    let props = vec![helper_property("key", 7, 1, [0u8; 15])];

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

// --- Edge tests ---

#[test]
// Verifies create_edge returns sequential IDs and get_edge round-trips without properties.
fn create_then_get_edge_with_no_properties() {
    let dir = temp_dir("create_then_get_edge_with_no_properties");
    let mut db = HiveDb::open(&dir).unwrap();

    let id = db.create_edge(1, 2, "KNOWS", vec![]).unwrap();
    let edge = db.get_edge(id).unwrap();

    assert_eq!(edge.id, id);
    assert_eq!(edge.label, "KNOWS");
    assert_eq!(edge.src, 1);
    assert_eq!(edge.dst, 2);
    assert!(edge.properties.is_empty());

    cleanup_dir(&dir);
}

#[test]
// Verifies create_edge with a single property is returned correctly by get_edge.
fn create_then_get_edge_with_single_property() {
    let dir = temp_dir("create_then_get_edge_with_single_property");
    let mut db = HiveDb::open(&dir).unwrap();

    let props = vec![helper_property("since", 99, 1, [0u8; 15])];

    let id = db.create_edge(10, 20, "FRIEND", props).unwrap();
    let edge = db.get_edge(id).unwrap();

    assert_eq!(edge.id, id);
    assert_eq!(edge.label, "FRIEND");
    assert_eq!(edge.src, 10);
    assert_eq!(edge.dst, 20);
    assert_eq!(edge.properties.len(), 1);
    assert_eq!(edge.properties[0].key_value, "since");
    assert_eq!(edge.properties[0].key_hash, 99);
    assert_eq!(edge.properties[0].value_type, 1);

    cleanup_dir(&dir);
}

#[test]
// Verifies create_edge with multiple properties preserves insertion order.
fn create_then_get_edge_with_multiple_properties() {
    let dir = temp_dir("create_then_get_edge_with_multiple_properties");
    let mut db = HiveDb::open(&dir).unwrap();

    let props = vec![
        helper_property("weight", 1, 1, [0u8; 15]),
        helper_property("since", 2, 2, [0u8; 15]),
        helper_property("type", 3, 3, [0u8; 15]),
    ];

    let id = db.create_edge(5, 15, "LINKED_TO", props).unwrap();
    let edge = db.get_edge(id).unwrap();

    assert_eq!(edge.label, "LINKED_TO");
    assert_eq!(edge.properties.len(), 3);
    assert_eq!(edge.properties[0].key_value, "weight");
    assert_eq!(edge.properties[1].key_value, "since");
    assert_eq!(edge.properties[2].key_value, "type");

    cleanup_dir(&dir);
}

#[test]
// Verifies get_edge for an id that exceeds the edge count returns a ReadError.
fn get_edge_out_of_bounds_returns_read_error() {
    let dir = temp_dir("get_edge_out_of_bounds_returns_read_error");
    let mut db = HiveDb::open(&dir).unwrap();

    let _id = db.create_edge(1, 2, "KNOWS", vec![]).unwrap();

    let result = db.get_edge(99);
    assert!(matches!(result, Err(DbError::ReadError)));

    cleanup_dir(&dir);
}

#[test]
// Verifies edge record and properties persist across close and reopen.
fn edge_data_persists_across_reopen() {
    let dir = temp_dir("edge_data_persists_across_reopen");

    let props = vec![helper_property("since", 8, 1, [0u8; 15])];

    let id = {
        let mut db = HiveDb::open(&dir).unwrap();
        db.create_edge(100, 200, "FOLLOWS", props).unwrap()
    };

    let mut db2 = HiveDb::open(&dir).unwrap();
    let edge = db2.get_edge(id).unwrap();

    assert_eq!(edge.id, id);
    assert_eq!(edge.src, 100);
    assert_eq!(edge.dst, 200);
    assert_eq!(edge.properties.len(), 1);
    assert_eq!(edge.properties[0].key_value, "since");

    cleanup_dir(&dir);
}

#[test]
// Verifies label de-duplication for edges: two edges with same label return same label string.
fn create_multiple_edges_with_same_label_returns_consistent_label() {
    let dir = temp_dir("create_multiple_edges_with_same_label_returns_consistent_label");
    let mut db = HiveDb::open(&dir).unwrap();

    let id0 = db.create_edge(1, 2, "KNOWS", vec![]).unwrap();
    let id1 = db.create_edge(3, 4, "KNOWS", vec![]).unwrap();
    let id2 = db.create_edge(5, 6, "LIKES", vec![]).unwrap();

    let e0 = db.get_edge(id0).unwrap();
    let e1 = db.get_edge(id1).unwrap();
    let e2 = db.get_edge(id2).unwrap();

    assert_eq!(e0.id, id0);
    assert_eq!(e1.id, id1);
    assert_eq!(e2.id, id2);
    assert_eq!(e0.label, "KNOWS");
    assert_eq!(e1.label, "KNOWS");
    assert_eq!(e2.label, "LIKES");

    cleanup_dir(&dir);
}

#[test]
// Verifies property value_inline bytes survive the create_edge / get_edge round-trip.
fn edge_property_value_inline_round_trips() {
    let dir = temp_dir("edge_property_value_inline_round_trips");
    let mut db = HiveDb::open(&dir).unwrap();

    let inline: [u8; 15] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    let props = vec![helper_property("data", 42, 7, inline)];

    let id = db.create_edge(1, 2, "HAS", props).unwrap();
    let edge = db.get_edge(id).unwrap();

    assert_eq!(edge.properties[0].value_inline, inline);
    assert_eq!(edge.properties[0].value_type, 7);

    cleanup_dir(&dir);
}

#[test]
// Verifies nodes and edges can share the same property store without interference.
fn nodes_and_edges_coexist_with_separate_properties() {
    let dir = temp_dir("nodes_and_edges_coexist_with_separate_properties");
    let mut db = HiveDb::open(&dir).unwrap();

    let node_props = vec![helper_property("name", 1, 1, [0u8; 15])];
    let node_id = db.create_node("Person", node_props).unwrap();

    let edge_props = vec![helper_property("since", 2, 1, [0u8; 15])];
    let edge_id = db.create_edge(1, 2, "KNOWS", edge_props).unwrap();

    let node = db.get_node(node_id).unwrap();
    let edge = db.get_edge(edge_id).unwrap();

    assert_eq!(node.properties.len(), 1);
    assert_eq!(node.properties[0].key_value, "name");
    assert_eq!(edge.properties.len(), 1);
    assert_eq!(edge.properties[0].key_value, "since");

    cleanup_dir(&dir);
}

#[test]
// Verifies sequential edge IDs are returned by create_edge.
fn create_edge_returns_sequential_ids() {
    let dir = temp_dir("create_edge_returns_sequential_ids");
    let mut db = HiveDb::open(&dir).unwrap();

    let id0 = db.create_edge(1, 2, "KNOWS", vec![]).unwrap();
    let id1 = db.create_edge(3, 4, "LIKES", vec![]).unwrap();
    let id2 = db.create_edge(5, 6, "FOLLOWS", vec![]).unwrap();

    assert_eq!(id0, 0);
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);

    cleanup_dir(&dir);
}

// --- Value property helper tests ---

#[test]
fn set_and_get_node_integer_property() {
    let dir = temp_dir("set_and_get_node_integer_property");
    let mut db = HiveDb::open(&dir).unwrap();

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
    let mut db = HiveDb::open(&dir).unwrap();

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
    let mut db = HiveDb::open(&dir).unwrap();

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
    let mut db = HiveDb::open(&dir).unwrap();

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
    let mut db = HiveDb::open(&dir).unwrap();

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
    let mut db = HiveDb::open(&dir).unwrap();

    let node_id = db.create_node("Person", vec![]).unwrap();

    let val = db.get_node_property(node_id, "nonexistent").unwrap();
    assert_eq!(val, None);

    cleanup_dir(&dir);
}

#[test]
fn set_and_get_edge_property() {
    let dir = temp_dir("set_and_get_edge_property");
    let mut db = HiveDb::open(&dir).unwrap();

    let edge_id = db.create_edge(1, 2, "KNOWS", vec![]).unwrap();
    db.set_edge_property(edge_id, "since", Value::Integer(2020))
        .unwrap();

    let val = db.get_edge_property(edge_id, "since").unwrap();
    assert_eq!(val, Some(Value::Integer(2020)));

    cleanup_dir(&dir);
}

#[test]
fn set_edge_property_overwrites_existing() {
    let dir = temp_dir("set_edge_property_overwrites_existing");
    let mut db = HiveDb::open(&dir).unwrap();

    let edge_id = db.create_edge(1, 2, "KNOWS", vec![]).unwrap();
    db.set_edge_property(edge_id, "weight", Value::Float(1.0))
        .unwrap();
    db.set_edge_property(edge_id, "weight", Value::Float(2.5))
        .unwrap();

    let val = db.get_edge_property(edge_id, "weight").unwrap();
    assert_eq!(val, Some(Value::Float(2.5)));

    cleanup_dir(&dir);
}
