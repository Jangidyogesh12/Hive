use super::super::utils::utils::{cleanup_dir, helper_property, temp_dir};
use crate::errors::DbError;
use crate::store::header::{self, DbHeader, HIVE_MAGIC};
use crate::value::Value;
use std::fs;
use std::io::Write;

#[test]
fn fresh_open_writes_header_with_magic_and_version() {
    let dir = temp_dir("fresh_open_writes_header");
    let _db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let meta_path = dir.join("meta.hive");
    let header = header::read_header(&meta_path).unwrap();
    assert_eq!(header.magic, HIVE_MAGIC);
    assert_eq!(header.version, 1);
    assert_eq!(header.node_count, 0);
    assert_eq!(header.edge_count, 0);
    assert_eq!(header.property_count, 0);

    cleanup_dir(&dir);
}

#[test]
fn create_node_updates_node_count() {
    let dir = temp_dir("create_node_updates_node_count");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    db.create_node("A", vec![]).unwrap();
    db.create_node("B", vec![]).unwrap();
    db.create_node("C", vec![]).unwrap();

    let meta_path = dir.join("meta.hive");
    let header = header::read_header(&meta_path).unwrap();
    assert_eq!(header.node_count, 3);
    assert_eq!(header.edge_count, 0);
    assert_eq!(header.property_count, 0);

    cleanup_dir(&dir);
}

#[test]
fn create_edge_updates_edge_count() {
    let dir = temp_dir("create_edge_updates_edge_count");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    db.create_edge(b, a, "KNOWS", vec![]).unwrap();

    let meta_path = dir.join("meta.hive");
    let header = header::read_header(&meta_path).unwrap();
    assert_eq!(header.node_count, 2);
    assert_eq!(header.edge_count, 2);

    cleanup_dir(&dir);
}

#[test]
fn delete_node_decrements_node_count() {
    let dir = temp_dir("delete_node_decrements_node_count");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    db.create_node("B", vec![]).unwrap();
    db.delete_node(a).unwrap();

    let meta_path = dir.join("meta.hive");
    let header = header::read_header(&meta_path).unwrap();
    assert_eq!(header.node_count, 1);

    cleanup_dir(&dir);
}

#[test]
fn delete_edge_decrements_edge_count() {
    let dir = temp_dir("delete_edge_decrements_edge_count");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db.create_node("A", vec![]).unwrap();
    let b = db.create_node("B", vec![]).unwrap();
    let e = db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    db.delete_edge(e).unwrap();

    let meta_path = dir.join("meta.hive");
    let header = header::read_header(&meta_path).unwrap();
    assert_eq!(header.node_count, 2);
    assert_eq!(header.edge_count, 0);

    cleanup_dir(&dir);
}

#[test]
fn property_count_tracks_creates_and_sets() {
    let dir = temp_dir("property_count_tracks");
    let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();

    let a = db
        .create_node(
            "Person",
            vec![helper_property("name", 1, 1, [0u8; 15])],
        )
        .unwrap();

    db.set_node_property(a, "age", Value::Integer(30)).unwrap();

    let meta_path = dir.join("meta.hive");
    let header = header::read_header(&meta_path).unwrap();
    assert_eq!(header.property_count, 2);

    cleanup_dir(&dir);
}

#[test]
fn header_survives_close_and_reopen() {
    let dir = temp_dir("header_survives_close_reopen");

    {
        let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();
        db.create_node("Person", vec![]).unwrap();
        db.create_node("Person", vec![]).unwrap();
        let a = db.create_node("A", vec![]).unwrap();
        let b = db.create_node("B", vec![]).unwrap();
        db.create_edge(a, b, "KNOWS", vec![]).unwrap();
    }

    {
        let meta_path = dir.join("meta.hive");
        let header = header::read_header(&meta_path).unwrap();
        assert_eq!(header.magic, HIVE_MAGIC);
        assert_eq!(header.version, 1);
        assert_eq!(header.node_count, 4);
        assert_eq!(header.edge_count, 1);
        assert_eq!(header.property_count, 0);
    }

    cleanup_dir(&dir);
}

#[test]
fn invalid_magic_rejected_on_open() {
    let dir = temp_dir("invalid_magic_rejected");
    let meta_path = dir.join("meta.hive");

    let mut file = fs::File::create(&meta_path).unwrap();
    let mut buf = DbHeader::new().to_bytes();
    buf[0] = b'X';
    file.write_all(&buf).unwrap();
    file.flush().unwrap();

    let result = crate::db::hive_db::HiveDb::open(&dir);
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, DbError::InvalidHeader));
    }

    cleanup_dir(&dir);
}

#[test]
fn unsupported_version_rejected_on_open() {
    let dir = temp_dir("unsupported_version_rejected");
    let meta_path = dir.join("meta.hive");

    let mut h = DbHeader::new();
    h.version = 99;
    header::write_header(&meta_path, h).unwrap();

    let result = crate::db::hive_db::HiveDb::open(&dir);
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, DbError::UnsupportedVersion));
    }

    cleanup_dir(&dir);
}

#[test]
fn reopen_still_accepts_valid_db() {
    let dir = temp_dir("reopen_still_accepts_valid_db");

    {
        let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();
        let a = db
            .create_node(
                "User",
                vec![helper_property("name", 42, 1, [0u8; 15])],
            )
            .unwrap();
        let b = db.create_node("User", vec![]).unwrap();
        db.create_edge(a, b, "FRIEND", vec![]).unwrap();
    }

    {
        let mut db = crate::db::hive_db::HiveDb::open(&dir).unwrap();
        let node_a = db.get_node(0).unwrap();
        assert_eq!(node_a.id, 0);
        assert_eq!(node_a.properties.len(), 1);
        assert_eq!(node_a.properties[0].key_value, "name");

        let node_b = db.get_node(1).unwrap();
        assert_eq!(node_b.id, 1);

        let out = db.get_out_neighbors(0).unwrap();
        assert_eq!(out, vec![1]);
    }

    cleanup_dir(&dir);
}
