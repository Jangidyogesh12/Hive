use super::super::utils::utils::{cleanup_dir, helper_property, temp_dir};

#[test]
fn create_then_get_edge_with_no_properties() {
    let dir = temp_dir("create_then_get_edge_with_no_properties");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let src = db.create_node("A", vec![]).unwrap();
    let dst = db.create_node("B", vec![]).unwrap();
    let id = db.create_edge(src, dst, "KNOWS", vec![]).unwrap();
    let edge = db.get_edge(id).unwrap();

    assert_eq!(edge.id, id);
    assert_eq!(edge.label, "KNOWS");
    assert_eq!(edge.src, src);
    assert_eq!(edge.dst, dst);
    assert!(edge.properties.is_empty());

    cleanup_dir(&dir);
}

#[test]
fn create_then_get_edge_with_single_property() {
    let dir = temp_dir("create_then_get_edge_with_single_property");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let props = vec![helper_property("since", 99, 1, [0u8; 15])];

    let src = db.create_node("A", vec![]).unwrap();
    let dst = db.create_node("B", vec![]).unwrap();
    let id = db.create_edge(src, dst, "FRIEND", props).unwrap();
    let edge = db.get_edge(id).unwrap();

    assert_eq!(edge.id, id);
    assert_eq!(edge.label, "FRIEND");
    assert_eq!(edge.src, src);
    assert_eq!(edge.dst, dst);
    assert_eq!(edge.properties.len(), 1);
    assert_eq!(edge.properties[0].key_value, "since");
    assert_eq!(edge.properties[0].key_hash, 99);
    assert_eq!(edge.properties[0].value_type, 1);

    cleanup_dir(&dir);
}

#[test]
fn create_then_get_edge_with_multiple_properties() {
    let dir = temp_dir("create_then_get_edge_with_multiple_properties");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let props = vec![
        helper_property("weight", 1, 1, [0u8; 15]),
        helper_property("since", 2, 2, [0u8; 15]),
        helper_property("type", 3, 3, [0u8; 15]),
    ];

    let src = db.create_node("A", vec![]).unwrap();
    let dst = db.create_node("B", vec![]).unwrap();
    let id = db.create_edge(src, dst, "LINKED_TO", props).unwrap();
    let edge = db.get_edge(id).unwrap();

    assert_eq!(edge.label, "LINKED_TO");
    assert_eq!(edge.properties.len(), 3);
    assert_eq!(edge.properties[0].key_value, "weight");
    assert_eq!(edge.properties[1].key_value, "since");
    assert_eq!(edge.properties[2].key_value, "type");

    cleanup_dir(&dir);
}

#[test]
fn get_edge_out_of_bounds_returns_read_error() {
    let dir = temp_dir("get_edge_out_of_bounds_returns_read_error");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let src = db.create_node("A", vec![]).unwrap();
    let dst = db.create_node("B", vec![]).unwrap();
    let _id = db.create_edge(src, dst, "KNOWS", vec![]).unwrap();

    let result = db.get_edge(99);
    assert!(matches!(result, Err(crate::errors::DbError::ReadError)));

    cleanup_dir(&dir);
}

#[test]
fn edge_data_persists_across_reopen() {
    let dir = temp_dir("edge_data_persists_across_reopen");

    let props = vec![helper_property("since", 8, 1, [0u8; 15])];

    let (id, src, dst) = {
        let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();
        let src = db.create_node("A", vec![]).unwrap();
        let dst = db.create_node("B", vec![]).unwrap();
        let id = db.create_edge(src, dst, "FOLLOWS", props).unwrap();
        (id, src, dst)
    };

    let mut db2 = crate::db::hive_db::HiveDb::open(&dir).unwrap();
    let edge = db2.get_edge(id).unwrap();

    assert_eq!(edge.id, id);
    assert_eq!(edge.src, src);
    assert_eq!(edge.dst, dst);
    assert_eq!(edge.properties.len(), 1);
    assert_eq!(edge.properties[0].key_value, "since");

    cleanup_dir(&dir);
}

#[test]
fn create_multiple_edges_with_same_label_returns_consistent_label() {
    let dir = temp_dir("create_multiple_edges_with_same_label_returns_consistent_label");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let c = db.create_node("C", vec![]).unwrap();
    let d = db.create_node("D", vec![]).unwrap();
    let e = db.create_node("E", vec![]).unwrap();
    let f = db.create_node("F", vec![]).unwrap();

    let id0 = db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    let id1 = db.create_edge(c, d, "KNOWS", vec![]).unwrap();
    let id2 = db.create_edge(e, f, "LIKES", vec![]).unwrap();

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
fn edge_property_value_inline_round_trips() {
    let dir = temp_dir("edge_property_value_inline_round_trips");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let inline: [u8; 15] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    let props = vec![helper_property("data", 42, 7, inline)];

    let src = db.create_node("A", vec![]).unwrap();
    let dst = db.create_node("B", vec![]).unwrap();
    let id = db.create_edge(src, dst, "HAS", props).unwrap();
    let edge = db.get_edge(id).unwrap();

    assert_eq!(edge.properties[0].value_inline, inline);
    assert_eq!(edge.properties[0].value_type, 7);

    cleanup_dir(&dir);
}

#[test]
fn nodes_and_edges_coexist_with_separate_properties() {
    let dir = temp_dir("nodes_and_edges_coexist_with_separate_properties");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let node_props = vec![helper_property("name", 1, 1, [0u8; 15])];
    let node_id = db.create_node("Person", node_props).unwrap();

    let edge_props = vec![helper_property("since", 2, 1, [0u8; 15])];
    let src_id = db.create_node("A", vec![]).unwrap();
    let dst_id = db.create_node("B", vec![]).unwrap();
    let edge_id = db.create_edge(src_id, dst_id, "KNOWS", edge_props).unwrap();

    let node = db.get_node(node_id).unwrap();
    let edge = db.get_edge(edge_id).unwrap();

    assert_eq!(node.properties.len(), 1);
    assert_eq!(node.properties[0].key_value, "name");
    assert_eq!(edge.properties.len(), 1);
    assert_eq!(edge.properties[0].key_value, "since");

    cleanup_dir(&dir);
}

#[test]
fn create_edge_returns_sequential_ids() {
    let dir = temp_dir("create_edge_returns_sequential_ids");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let c = db.create_node("C", vec![]).unwrap();
    let d = db.create_node("D", vec![]).unwrap();
    let e = db.create_node("E", vec![]).unwrap();
    let f = db.create_node("F", vec![]).unwrap();

    let id0 = db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    let id1 = db.create_edge(c, d, "LIKES", vec![]).unwrap();
    let id2 = db.create_edge(e, f, "FOLLOWS", vec![]).unwrap();

    assert_eq!(id0, 0);
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);

    cleanup_dir(&dir);
}
