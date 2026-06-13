use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::db::store_path::WAL_FILE;
use crate::value::Value;
use crate::wal::{Wal, WalEntry, WalProperty};
use std::fs;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

#[test]
fn wal_roundtrips_all_entry_types() {
    let dir = temp_dir("wal_roundtrips_all_entry_types");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    let entries = vec![
        WalEntry::CreateNode {
            node_id: 7,
            label: "Person".to_string(),
            properties: vec![
                WalProperty {
                    key: "name".to_string(),
                    value: Value::String("Alice".to_string()),
                },
                WalProperty {
                    key: "age".to_string(),
                    value: Value::Integer(30),
                },
            ],
        },
        WalEntry::CreateEdge {
            edge_id: 3,
            src: 7,
            dst: 8,
            label: "KNOWS".to_string(),
            properties: vec![WalProperty {
                key: "since".to_string(),
                value: Value::Integer(2020),
            }],
        },
        WalEntry::UpdateNode {
            node_id: 7,
            key: "score".to_string(),
            value: Value::Float(3.5),
        },
        WalEntry::UpdateEdge {
            edge_id: 3,
            key: "active".to_string(),
            value: Value::Boolean(true),
        },
        WalEntry::DeleteNode { node_id: 9 },
        WalEntry::DeleteEdge { edge_id: 11 },
        WalEntry::Checkpoint,
    ];

    for entry in &entries {
        wal.append(entry).unwrap();
    }

    let got = wal.read_all().unwrap();
    assert_eq!(got, entries);

    cleanup_dir(&dir);
}

#[test]
fn wal_truncate_clears_entries() {
    let dir = temp_dir("wal_truncate_clears_entries");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    wal.append(&WalEntry::DeleteNode { node_id: 1 }).unwrap();
    wal.truncate().unwrap();

    let got = wal.read_all().unwrap();
    assert!(got.is_empty());

    cleanup_dir(&dir);
}

#[test]
fn wal_bad_checksum_ignores_corrupt_tail() {
    let dir = temp_dir("wal_bad_checksum_ignores_corrupt_tail");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    let first = WalEntry::DeleteNode { node_id: 1 };
    let second = WalEntry::DeleteEdge { edge_id: 2 };

    wal.append(&first).unwrap();
    wal.append(&second).unwrap();

    let mut file = OpenOptions::new().read(true).write(true).open(&path).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[0xFF]).unwrap();
    file.flush().unwrap();

    let got = wal.read_all().unwrap();
    assert_eq!(got, vec![first]);

    cleanup_dir(&dir);
}

#[test]
fn wal_partial_last_entry_is_ignored() {
    let dir = temp_dir("wal_partial_last_entry_is_ignored");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    let first = WalEntry::Checkpoint;
    wal.append(&first).unwrap();

    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(&10u32.to_le_bytes()).unwrap();
    file.write_all(&[1, 2, 3]).unwrap();
    file.flush().unwrap();

    let got = wal.read_all().unwrap();
    assert_eq!(got, vec![first]);

    cleanup_dir(&dir);
}

#[test]
fn wal_sync_succeeds_after_append() {
    let dir = temp_dir("wal_sync_succeeds_after_append");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    wal.append(&WalEntry::UpdateNode {
        node_id: 1,
        key: "name".to_string(),
        value: Value::String("Alice".to_string()),
    })
    .unwrap();

    wal.sync().unwrap();

    cleanup_dir(&dir);
}

#[test]
fn hive_db_create_node_writes_wal_entry() {
    let dir = temp_dir("hive_db_create_node_writes_wal_entry");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        db.create_node("Person", vec![]).unwrap();
    }

    let mut wal = Wal::open(&dir.join(WAL_FILE)).unwrap();
    let entries = wal.read_all().unwrap();

    assert_eq!(
        entries,
        vec![
            WalEntry::CreateNode {
                node_id: 0,
                label: "Person".to_string(),
                properties: vec![],
            },
            WalEntry::Checkpoint,
        ]
    );

    cleanup_dir(&dir);
}

#[test]
fn hive_db_property_and_delete_operations_write_wal_entries() {
    let dir = temp_dir("hive_db_property_and_delete_operations_write_wal_entries");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let node_id = db.create_node("Person", vec![]).unwrap();
        db.set_node_property(node_id, "name", Value::String("Alice".to_string()))
            .unwrap();
        db.delete_node(node_id).unwrap();
    }

    let mut wal = Wal::open(&dir.join(WAL_FILE)).unwrap();
    let entries = wal.read_all().unwrap();

    assert_eq!(
        entries,
        vec![
            WalEntry::CreateNode {
                node_id: 0,
                label: "Person".to_string(),
                properties: vec![],
            },
            WalEntry::Checkpoint,
            WalEntry::UpdateNode {
                node_id: 0,
                key: "name".to_string(),
                value: Value::String("Alice".to_string()),
            },
            WalEntry::Checkpoint,
            WalEntry::DeleteNode { node_id: 0 },
            WalEntry::Checkpoint,
        ]
    );

    cleanup_dir(&dir);
}

#[test]
fn hive_db_edge_operations_write_wal_entries() {
    let dir = temp_dir("hive_db_edge_operations_write_wal_entries");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let src = db.create_node("A", vec![]).unwrap();
        let dst = db.create_node("B", vec![]).unwrap();
        let edge_id = db.create_edge(src, dst, "KNOWS", vec![]).unwrap();
        db.set_edge_property(edge_id, "since", Value::Integer(2020))
            .unwrap();
        db.delete_edge(edge_id).unwrap();
    }

    let mut wal = Wal::open(&dir.join(WAL_FILE)).unwrap();
    let entries = wal.read_all().unwrap();

    assert_eq!(
        entries,
        vec![
            WalEntry::CreateNode {
                node_id: 0,
                label: "A".to_string(),
                properties: vec![],
            },
            WalEntry::Checkpoint,
            WalEntry::CreateNode {
                node_id: 1,
                label: "B".to_string(),
                properties: vec![],
            },
            WalEntry::Checkpoint,
            WalEntry::CreateEdge {
                edge_id: 0,
                src: 0,
                dst: 1,
                label: "KNOWS".to_string(),
                properties: vec![],
            },
            WalEntry::Checkpoint,
            WalEntry::UpdateEdge {
                edge_id: 0,
                key: "since".to_string(),
                value: Value::Integer(2020),
            },
            WalEntry::Checkpoint,
            WalEntry::DeleteEdge { edge_id: 0 },
            WalEntry::Checkpoint,
        ]
    );

    cleanup_dir(&dir);
}

#[test]
fn hive_db_reopen_truncates_checkpointed_wal() {
    let dir = temp_dir("hive_db_reopen_truncates_checkpointed_wal");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let node_id = db.create_node("Person", vec![]).unwrap();
        db.set_node_property(node_id, "name", Value::String("Alice".to_string()))
            .unwrap();
        db.close();
    }

    let mut wal = Wal::open(&dir.join(WAL_FILE)).unwrap();
    let entries = wal.read_all().unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries.last(), Some(&WalEntry::Checkpoint));

    {
        let mut reopened = HiveDb::open(&dir).unwrap();
        let name = reopened.get_node_property(0, "name").unwrap();
        assert_eq!(name, Some(Value::String("Alice".to_string())));
    }

    let metadata = fs::metadata(dir.join(WAL_FILE)).unwrap();
    assert_eq!(metadata.len(), 0);

    let mut wal = Wal::open(&dir.join(WAL_FILE)).unwrap();
    assert!(wal.read_all().unwrap().is_empty());

    cleanup_dir(&dir);
}

#[test]
fn hive_db_open_keeps_dirty_wal() {
    let dir = temp_dir("hive_db_open_keeps_dirty_wal");

    {
        let mut wal = Wal::open(&dir.join(WAL_FILE)).unwrap();
        wal.append(&WalEntry::DeleteNode { node_id: 9 }).unwrap();
    }

    let _db = HiveDb::open(&dir).unwrap();

    let mut wal = Wal::open(&dir.join(WAL_FILE)).unwrap();
    assert_eq!(wal.read_all().unwrap(), vec![WalEntry::DeleteNode { node_id: 9 }]);

    cleanup_dir(&dir);
}
