// Tests for variable-width record serialization: Node, Edge, Property.
use crate::storage::page::record::{EdgeRecordV2, NodeRecordV2, PropertyEntry, PropertyRecordV2};
use crate::types::NIL_ID;
use crate::value::Value;

fn make_property(key_hash: u64, value: &Value) -> PropertyEntry {
    let (value_type, value_inline) = value.to_inline_bytes();
    PropertyEntry {
        key_hash,
        value_type,
        value_inline,
        long_value_offset: 0,
    }
}

#[test]
fn node_record_new_sets_defaults() {
    let node = NodeRecordV2::new(42);
    assert_eq!(node.id, 42);
    assert_eq!(node.label_id, 0);
    assert_eq!(node.flags, 0);
    assert_eq!(node.first_out_edge, NIL_ID);
    assert_eq!(node.first_in_edge, NIL_ID);
    assert_eq!(node.first_property, NIL_ID);
    assert!(node.properties.is_empty());
}

#[test]
fn node_record_no_properties_roundtrip() {
    let mut node = NodeRecordV2::new(100);
    node.label_id = 5;
    node.first_out_edge = 20;

    let size = node.encoded_size();
    let mut buf = vec![0u8; size];
    let written = node.to_bytes(&mut buf).unwrap();
    assert_eq!(written, size);

    let decoded = NodeRecordV2::from_bytes(&buf).unwrap();
    assert_eq!(decoded.id, 100);
    assert_eq!(decoded.label_id, 5);
    assert_eq!(decoded.first_out_edge, 20);
    assert_eq!(decoded.first_in_edge, NIL_ID);
    assert_eq!(decoded.flags, 0);
    assert_eq!(decoded.properties.len(), 0);
}

#[test]
fn node_record_with_integer_property_roundtrip() {
    let mut node = NodeRecordV2::new(1);
    node.properties
        .push(make_property(100, &Value::Integer(42)));

    let size = node.encoded_size();
    let mut buf = vec![0u8; size];
    node.to_bytes(&mut buf).unwrap();

    let decoded = NodeRecordV2::from_bytes(&buf).unwrap();
    assert_eq!(decoded.properties.len(), 1);
    assert_eq!(decoded.properties[0].key_hash, 100);
    assert_eq!(decoded.properties[0].value_type, crate::value::INTEGER);
    let val = Value::from_bytes(
        decoded.properties[0].value_type,
        decoded.properties[0].value_inline,
    );
    assert_eq!(val, Value::Integer(42));
}

#[test]
fn node_record_with_multiple_property_types_roundtrip() {
    let mut node = NodeRecordV2::new(50);
    node.properties.push(make_property(1, &Value::Null));
    node.properties.push(make_property(2, &Value::Integer(999)));
    node.properties.push(make_property(3, &Value::Float(3.14)));
    node.properties
        .push(make_property(4, &Value::Boolean(true)));
    node.properties
        .push(make_property(5, &Value::String("hello".into())));

    let size = node.encoded_size();
    let mut buf = vec![0u8; size];
    node.to_bytes(&mut buf).unwrap();

    let decoded = NodeRecordV2::from_bytes(&buf).unwrap();
    assert_eq!(decoded.properties.len(), 5);

    let types = [
        crate::value::NULL,
        crate::value::INTEGER,
        crate::value::FLOAT,
        crate::value::BOOLEAN,
        crate::value::STRING,
    ];
    for (i, &expected_type) in types.iter().enumerate() {
        assert_eq!(
            decoded.properties[i].value_type, expected_type,
            "property {} type mismatch",
            i
        );
    }

    let val4 = Value::from_bytes(
        decoded.properties[4].value_type,
        decoded.properties[4].value_inline,
    );
    assert_eq!(val4, Value::String("hello".into()));
}

#[test]
fn node_record_with_many_properties_roundtrip() {
    let mut node = NodeRecordV2::new(1);
    for i in 0..100 {
        node.properties
            .push(make_property(i as u64, &Value::Integer(i as i64)));
    }

    let size = node.encoded_size();
    let mut buf = vec![0u8; size];
    node.to_bytes(&mut buf).unwrap();

    let decoded = NodeRecordV2::from_bytes(&buf).unwrap();
    assert_eq!(decoded.properties.len(), 100);
    for i in 0..100 {
        assert_eq!(decoded.properties[i].key_hash, i as u64);
        let val = Value::from_bytes(
            decoded.properties[i].value_type,
            decoded.properties[i].value_inline,
        );
        assert_eq!(val, Value::Integer(i as i64));
    }
}

#[test]
fn edge_record_new_sets_defaults() {
    let edge = EdgeRecordV2::new(7);
    assert_eq!(edge.id, 7);
    assert_eq!(edge.label_id, 0);
    assert_eq!(edge.src, NIL_ID);
    assert_eq!(edge.dst, NIL_ID);
    assert_eq!(edge.next_out_edge, NIL_ID);
    assert_eq!(edge.next_in_edge, NIL_ID);
    assert_eq!(edge.first_property, NIL_ID);
    assert!(edge.properties.is_empty());
}

#[test]
fn edge_record_with_properties_roundtrip() {
    let mut edge = EdgeRecordV2::new(99);
    edge.src = 10;
    edge.dst = 20;
    edge.label_id = 3;
    edge.next_out_edge = 88;
    edge.flags = 1;
    edge.properties
        .push(make_property(50, &Value::Boolean(false)));
    edge.properties.push(make_property(51, &Value::Float(2.71)));

    let size = edge.encoded_size();
    let mut buf = vec![0u8; size];
    edge.to_bytes(&mut buf).unwrap();

    let decoded = EdgeRecordV2::from_bytes(&buf).unwrap();
    assert_eq!(decoded.id, 99);
    assert_eq!(decoded.src, 10);
    assert_eq!(decoded.dst, 20);
    assert_eq!(decoded.label_id, 3);
    assert_eq!(decoded.next_out_edge, 88);
    assert_eq!(decoded.flags, 1);
    assert_eq!(decoded.properties.len(), 2);

    let val0 = Value::from_bytes(
        decoded.properties[0].value_type,
        decoded.properties[0].value_inline,
    );
    assert_eq!(val0, Value::Boolean(false));
}

#[test]
fn edge_record_different_from_node_record() {
    let mut node = NodeRecordV2::new(1);
    node.properties.push(make_property(1, &Value::Integer(10)));

    let mut edge = EdgeRecordV2::new(1);
    edge.src = 5;
    edge.dst = 6;
    edge.properties.push(make_property(1, &Value::Integer(10)));

    let node_size = node.encoded_size();
    let edge_size = edge.encoded_size();
    assert!(edge_size > node_size, "edge should be larger than node");
}

#[test]
fn property_record_roundtrip_all_fields() {
    let mut prop = PropertyRecordV2::new(42);
    prop.key_hash = 0xABCD;
    prop.key_offset = 500;
    prop.value_type = crate::value::INTEGER;
    prop.value_inline[..8].copy_from_slice(&999i64.to_le_bytes());
    prop.next_property = 200;
    prop.flags = 1;

    let mut buf = [0u8; PropertyRecordV2::SIZE];
    let written = prop.to_bytes(&mut buf).unwrap();
    assert_eq!(written, PropertyRecordV2::SIZE);

    let decoded = PropertyRecordV2::from_bytes(&buf).unwrap();
    assert_eq!(decoded.id, 42);
    assert_eq!(decoded.key_hash, 0xABCD);
    assert_eq!(decoded.key_offset, 500);
    assert_eq!(decoded.value_type, crate::value::INTEGER);
    assert_eq!(decoded.next_property, 200);
    assert_eq!(decoded.flags, 1);
}

#[test]
fn property_record_string_value_roundtrip() {
    let mut prop = PropertyRecordV2::new(1);
    let (vt, inline) = Value::String("short".into()).to_inline_bytes();
    prop.value_type = vt;
    prop.value_inline = inline;

    let mut buf = [0u8; PropertyRecordV2::SIZE];
    prop.to_bytes(&mut buf).unwrap();

    let decoded = PropertyRecordV2::from_bytes(&buf).unwrap();
    assert_eq!(decoded.value_type, crate::value::STRING);
    let val = Value::from_bytes(decoded.value_type, decoded.value_inline);
    assert_eq!(val, Value::String("short".into()));
}

#[test]
fn property_record_encoded_size_is_constant() {
    let prop = PropertyRecordV2::new(1);
    assert_eq!(prop.encoded_size(), PropertyRecordV2::SIZE);
}
