use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::db::store_path::WAL_FILE;
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
    assert_eq!(reopened.node_count().unwrap(), 0);

    cleanup_dir(&dir);
}

#[test]
fn transaction_commit_writes_grouped_wal_entry() {
    let dir = temp_dir("transaction_commit_writes_grouped_wal_entry");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let mut tx = db.begin().unwrap();
        let node_id = tx.create_node("Person", vec![]).unwrap();
        tx.set_node_property(node_id, "name", Value::String("Alice".to_string()))
            .unwrap();
        tx.commit().unwrap();
    }

    let mut wal = Wal::open(&dir.join(WAL_FILE)).unwrap();
    let entries = wal.read_all().unwrap();

    assert_eq!(entries.len(), 2);
    assert!(matches!(entries[1], WalEntry::Checkpoint));

    match &entries[0] {
        WalEntry::Transaction { entries } => {
            assert_eq!(entries.len(), 2);
            assert!(matches!(entries[0], WalEntry::CreateNode { .. }));
            assert!(matches!(entries[1], WalEntry::UpdateNode { .. }));
        }
        other => panic!("expected transaction entry, got {other:?}"),
    }

    cleanup_dir(&dir);
}
