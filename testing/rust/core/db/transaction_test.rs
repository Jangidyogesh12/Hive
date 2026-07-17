use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::value::Value;
use crate::wal::{Wal, WalEntry};

#[test]
fn transaction_commit_persists_across_reopen() {
    let dir = temp_dir("transaction_commit_persists_across_reopen");

    let node_id = {
        let mut db = HiveDb::open(&dir).unwrap();
        let mut tx = db.begin().unwrap();
        let node_id = tx.create_node("Person", vec![]).unwrap();
        tx.set_node_property(node_id, "name", Value::String("Alice".to_string()))
            .unwrap();
        tx.commit().unwrap();
        node_id
    };

    let mut reopened = HiveDb::open(&dir).unwrap();
    let node = reopened.get_node(node_id).unwrap();

    assert_eq!(node.label, "Person");
    assert_eq!(
        reopened.get_node_property(node_id, "name").unwrap(),
        Some(Value::String("Alice".to_string()))
    );

    cleanup_dir(&dir);
}

#[test]
fn transaction_rollback_discards_buffered_ops() {
    let dir = temp_dir("transaction_rollback_discards_buffered_ops");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let mut tx = db.begin().unwrap();
        let node_id = tx.create_node("Person", vec![]).unwrap();
        tx.set_node_property(node_id, "name", Value::String("Alice".to_string()))
            .unwrap();
        tx.rollback();
    }

    let mut reopened = HiveDb::open(&dir).unwrap();
    assert!(reopened.node_count().unwrap() >= 1);

    cleanup_dir(&dir);
}

#[test]
fn transaction_commit_writes_physical_wal_entries() {
    let dir = temp_dir("transaction_commit_writes_physical_wal_entries");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let mut tx = db.begin().unwrap();
        let node_id = tx.create_node("Person", vec![]).unwrap();
        tx.set_node_property(node_id, "name", Value::String("Alice".to_string()))
            .unwrap();
        tx.commit().unwrap();
    }

    let mut wal = Wal::open(&dir.join("wal.hive")).unwrap();
    let entries = wal.read_all().unwrap();

    assert!(entries.len() >= 2);

    let has_begin = entries.iter().any(|e| matches!(e, WalEntry::Begin { .. }));
    let has_checkpoint = entries
        .iter()
        .any(|e| matches!(e, WalEntry::Checkpoint { .. }));
    assert!(has_begin, "WAL should contain Begin entry");
    assert!(
        has_checkpoint,
        "WAL should contain Checkpoint entry"
    );

    for entry in &entries {
        match entry {
            WalEntry::Begin { tx_id, lsn } => {
                assert!(*tx_id > 0);
                assert!(*lsn > 0);
            }
            WalEntry::PageImage {
                tx_id, lsn, bytes, ..
            } => {
                assert!(*tx_id > 0);
                assert!(*lsn > 0);
                assert_eq!(bytes.len(), 4096);
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
