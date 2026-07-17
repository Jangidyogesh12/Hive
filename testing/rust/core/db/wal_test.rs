use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::db::store_path::WAL_FILE;
use crate::storage::page::format::PAGE_SIZE;
use crate::storage::pager::FileId;
use crate::value::Value;
use crate::wal::{Wal, WalEntry};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

#[test]
fn wal_roundtrips_physical_entry_types() {
    let dir = temp_dir("wal_roundtrips_physical_entry_types");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    let mut page_data = [0u8; PAGE_SIZE];
    page_data[0] = 0x01;
    page_data[1] = 0x02;

    let entries = vec![
        WalEntry::Begin {
            tx_id: 1,
            lsn: 1,
        },
        WalEntry::PageImage {
            tx_id: 1,
            lsn: 2,
            file_id: FileId::Nodes,
            page_id: 0,
            page_lsn: 2,
            bytes: Box::new(page_data),
        },
        WalEntry::Commit { tx_id: 1, lsn: 3 },
        WalEntry::Checkpoint { lsn: 4 },
    ];

    for entry in &entries {
        wal.append(entry).unwrap();
    }

    let got = wal.read_all().unwrap();
    assert_eq!(got.len(), entries.len());

    assert!(matches!(got[0], WalEntry::Begin { tx_id: 1, lsn: 1 }));
    match &got[1] {
        WalEntry::PageImage {
            tx_id,
            lsn,
            file_id,
            page_id,
            page_lsn,
            bytes,
        } => {
            assert_eq!(*tx_id, 1);
            assert_eq!(*lsn, 2);
            assert_eq!(*file_id, FileId::Nodes);
            assert_eq!(*page_id, 0);
            assert_eq!(*page_lsn, 2);
            assert_eq!(bytes[0], 0x01);
            assert_eq!(bytes[1], 0x02);
        }
        _ => panic!("Expected PageImage"),
    }
    assert!(matches!(got[2], WalEntry::Commit { tx_id: 1, lsn: 3 }));
    assert!(matches!(got[3], WalEntry::Checkpoint { lsn: 4 }));

    cleanup_dir(&dir);
}

#[test]
fn wal_truncate_clears_entries() {
    let dir = temp_dir("wal_truncate_clears_entries");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    wal.append(&WalEntry::Begin {
        tx_id: 1,
        lsn: 1,
    })
    .unwrap();
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

    let first = WalEntry::Begin {
        tx_id: 1,
        lsn: 1,
    };
    let second = WalEntry::Commit { tx_id: 1, lsn: 2 };

    wal.append(&first).unwrap();
    wal.append(&second).unwrap();

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[0xFF]).unwrap();
    file.flush().unwrap();

    let got = wal.read_all().unwrap();
    assert_eq!(got.len(), 1);
    assert!(matches!(got[0], WalEntry::Begin { tx_id: 1, lsn: 1 }));

    cleanup_dir(&dir);
}

#[test]
fn wal_partial_last_entry_is_ignored() {
    let dir = temp_dir("wal_partial_last_entry_is_ignored");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    let first = WalEntry::Checkpoint { lsn: 1 };
    wal.append(&first).unwrap();

    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(&10u32.to_le_bytes()).unwrap();
    file.write_all(&[1, 2, 3]).unwrap();
    file.flush().unwrap();

    let got = wal.read_all().unwrap();
    assert_eq!(got.len(), 1);
    assert!(matches!(got[0], WalEntry::Checkpoint { lsn: 1 }));

    cleanup_dir(&dir);
}

#[test]
fn wal_sync_succeeds_after_append() {
    let dir = temp_dir("wal_sync_succeeds_after_append");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    wal.append(&WalEntry::Begin {
        tx_id: 1,
        lsn: 1,
    })
    .unwrap();

    wal.sync().unwrap();

    cleanup_dir(&dir);
}

#[test]
fn hive_db_create_node_writes_physical_wal() {
    let dir = temp_dir("hive_db_create_node_writes_physical_wal");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        db.create_node("Person", vec![]).unwrap();
    }

    let mut wal = Wal::open(&dir.join(WAL_FILE)).unwrap();
    let entries = wal.read_all().unwrap();

    let has_begin = entries.iter().any(|e| matches!(e, WalEntry::Begin { .. }));
    let has_checkpoint = entries
        .iter()
        .any(|e| matches!(e, WalEntry::Checkpoint { .. }));
    assert!(has_begin, "WAL should contain Begin entry");
    assert!(has_checkpoint, "WAL should contain Checkpoint entry");

    cleanup_dir(&dir);
}

#[test]
fn hive_db_property_and_delete_operations_write_physical_wal() {
    let dir = temp_dir("hive_db_property_and_delete_operations_write_physical_wal");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let node_id = db.create_node("Person", vec![]).unwrap();
        db.set_node_property(node_id, "name", Value::String("Alice".to_string()))
            .unwrap();
        db.delete_node(node_id).unwrap();
    }

    let mut wal = Wal::open(&dir.join(WAL_FILE)).unwrap();
    let entries = wal.read_all().unwrap();

    let has_checkpoint = entries
        .iter()
        .any(|e| matches!(e, WalEntry::Checkpoint { .. }));
    assert!(
        has_checkpoint,
        "WAL should contain checkpoint entries"
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

    {
        let mut reopened = HiveDb::open(&dir).unwrap();
        let name = reopened.get_node_property(0, "name").unwrap();
        assert_eq!(name, Some(Value::String("Alice".to_string())));
    }

    cleanup_dir(&dir);
}

#[test]
fn hive_db_crash_recovery_preserves_data() {
    let dir = temp_dir("hive_db_crash_recovery_preserves_data");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        db.create_node("Person", vec![]).unwrap();
        db.create_node("Car", vec![]).unwrap();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let node0 = db.get_node(0).unwrap();
        assert_eq!(node0.label, "Person");
        let node1 = db.get_node(1).unwrap();
        assert_eq!(node1.label, "Car");
    }

    cleanup_dir(&dir);
}

#[test]
fn hive_db_wal_entries_are_valid_physical() {
    let dir = temp_dir("hive_db_wal_entries_are_valid_physical");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        db.create_node("Person", vec![]).unwrap();
        db.create_node("Car", vec![]).unwrap();
    }

    let mut wal = Wal::open(&dir.join(WAL_FILE)).unwrap();
    let entries = wal.read_all().unwrap();

    for entry in &entries {
        match entry {
            WalEntry::Begin { tx_id, lsn } => {
                assert!(*tx_id > 0);
                assert!(*lsn > 0);
            }
            WalEntry::PageImage {
                tx_id,
                lsn,
                file_id,
                page_id: _,
                page_lsn,
                bytes,
            } => {
                assert!(*tx_id > 0);
                assert!(*lsn > 0);
                assert!(FileId::from_u8(*file_id as u8).is_some());
                assert!(*page_lsn > 0);
                assert_eq!(bytes.len(), PAGE_SIZE);
            }
            WalEntry::Commit { tx_id, lsn } => {
                assert!(*tx_id > 0);
                assert!(*lsn > 0);
            }
            WalEntry::Checkpoint { lsn } => {
                assert!(*lsn > 0);
            }
        }
    }

    cleanup_dir(&dir);
}
