use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::value::Value;
use crate::wal::{Wal, WalEntry};

#[test]
fn create_node_writes_wal_entries() {
    let dir = temp_dir("create_node_writes_wal");
    let mut db = HiveDb::open(&dir).unwrap();

    let _ = db.create_node().unwrap();

    let mut wal = Wal::open(&dir.join("wal.hive")).unwrap();
    let entries = wal.read_all().unwrap();

    let begins: Vec<_> = entries
        .iter()
        .filter(|e| matches!(e, WalEntry::Begin { .. }))
        .collect();
    let page_images: Vec<_> = entries
        .iter()
        .filter(|e| matches!(e, WalEntry::PageImage { .. }))
        .collect();
    let commits: Vec<_> = entries
        .iter()
        .filter(|e| matches!(e, WalEntry::Commit { .. }))
        .collect();

    assert_eq!(begins.len(), 1, "expected one Begin entry");
    assert!(page_images.len() >= 1, "expected at least one PageImage");
    assert_eq!(commits.len(), 1, "expected one Commit entry");

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_property_writes_wal_entries() {
    let dir = temp_dir("set_property_writes_wal");
    let mut db = HiveDb::open(&dir).unwrap();

    let node_id = db.create_node().unwrap();
    db.set_node_property(node_id, "name", &Value::String("alice".into()))
        .unwrap();

    let mut wal = Wal::open(&dir.join("wal.hive")).unwrap();
    let entries = wal.read_all().unwrap();

    let begins: Vec<_> = entries
        .iter()
        .filter(|e| matches!(e, WalEntry::Begin { .. }))
        .collect();
    assert_eq!(
        begins.len(),
        2,
        "expected two Begin entries (create + set_property)"
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_node_survives_crash_recovery() {
    let dir = temp_dir("crash_recovery_node");

    let node_id;
    {
        let mut db = HiveDb::open(&dir).unwrap();
        node_id = db.create_node().unwrap();
        let _ = db.create_node().unwrap();
        let _ = db.create_node().unwrap();
        drop(db);
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let node = db.get_node(node_id).unwrap();
        assert_eq!(node.id, 1);
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn set_property_survives_crash_recovery() {
    let dir = temp_dir("crash_recovery_property");

    let node_id;
    {
        let mut db = HiveDb::open(&dir).unwrap();
        node_id = db.create_node().unwrap();
        db.set_node_property(node_id, "name", &Value::String("alice".into()))
            .unwrap();
        drop(db);
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let val = db.get_node_property(node_id, "name").unwrap();
        assert_eq!(val, Value::String("alice".into()));
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn edge_survives_crash_recovery() {
    let dir = temp_dir("crash_recovery_edge");

    let src_id;
    let dst_id;
    let edge_id;
    {
        let mut db = HiveDb::open(&dir).unwrap();
        src_id = db.create_node().unwrap();
        dst_id = db.create_node().unwrap();
        edge_id = db.create_edge(src_id, dst_id).unwrap();
        drop(db);
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let edge = db.get_edge(edge_id).unwrap();
        assert_eq!(edge.src, src_id);
        assert_eq!(edge.dst, dst_id);
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn checkpoint_truncates_wal() {
    let dir = temp_dir("checkpoint_truncates_wal");
    let mut db = HiveDb::open(&dir).unwrap();

    let _ = db.create_node().unwrap();
    let _ = db.create_node().unwrap();

    {
        let mut wal = Wal::open(&dir.join("wal.hive")).unwrap();
        let entries = wal.read_all().unwrap();
        assert!(
            !entries.is_empty(),
            "WAL should have entries before checkpoint"
        );
    }

    db.checkpoint().unwrap();

    {
        let mut wal = Wal::open(&dir.join("wal.hive")).unwrap();
        let entries = wal.read_all().unwrap();
        assert!(entries.is_empty(), "WAL should be empty after checkpoint");
    }

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn data_survives_after_checkpoint_and_reopen() {
    let dir = temp_dir("checkpoint_and_reopen");

    let node_id;
    let edge_id;
    {
        let mut db = HiveDb::open(&dir).unwrap();
        let src = db.create_node().unwrap();
        let dst = db.create_node().unwrap();
        edge_id = db.create_edge(src, dst).unwrap();
        node_id = src;
        db.set_node_property(node_id, "key", &Value::Integer(42))
            .unwrap();
        db.checkpoint().unwrap();
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let node = db.get_node(node_id).unwrap();
        assert_eq!(node.id, 1);

        let edge = db.get_edge(edge_id).unwrap();
        assert_eq!(edge.src, node_id);

        let val = db.get_node_property(node_id, "key").unwrap();
        assert_eq!(val, Value::Integer(42));
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn multiple_operations_wal_entries_have_sequential_lsns() {
    let dir = temp_dir("wal_sequential_lsns");
    let mut db = HiveDb::open(&dir).unwrap();

    let _ = db.create_node().unwrap();
    let _ = db.create_node().unwrap();
    let _ = db.create_node().unwrap();

    let mut wal = Wal::open(&dir.join("wal.hive")).unwrap();
    let entries = wal.read_all().unwrap();

    let mut prev_lsn = 0u64;
    for entry in &entries {
        let lsn = entry.lsn();
        assert!(lsn > prev_lsn, "LSNs should be strictly increasing");
        prev_lsn = lsn;
    }

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_edge_writes_wal_entries() {
    let dir = temp_dir("create_edge_writes_wal");
    let mut db = HiveDb::open(&dir).unwrap();

    let src = db.create_node().unwrap();
    let dst = db.create_node().unwrap();
    let _ = db.create_edge(src, dst).unwrap();

    let mut wal = Wal::open(&dir.join("wal.hive")).unwrap();
    let entries = wal.read_all().unwrap();

    let begins: Vec<_> = entries
        .iter()
        .filter(|e| matches!(e, WalEntry::Begin { .. }))
        .collect();
    assert_eq!(
        begins.len(),
        3,
        "expected three Begin entries (2 nodes + 1 edge)"
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn explicit_transaction_commits_multiple_operations_once() {
    let dir = temp_dir("explicit_transaction_commit");
    let mut db = HiveDb::open(&dir).unwrap();

    let node_id;
    let edge_id;
    {
        let mut tx = db.begin().unwrap();
        let src = tx.create_node().unwrap();
        let dst = tx.create_node().unwrap();
        edge_id = tx.create_edge(src, dst).unwrap();
        node_id = src;
        tx.set_node_property(node_id, "name", &Value::String("alice".into()))
            .unwrap();
        tx.commit().unwrap();
    }

    let mut wal = Wal::open(&dir.join("wal.hive")).unwrap();
    let entries = wal.read_all().unwrap();
    let begins = entries
        .iter()
        .filter(|e| matches!(e, WalEntry::Begin { .. }))
        .count();
    let commits = entries
        .iter()
        .filter(|e| matches!(e, WalEntry::Commit { .. }))
        .count();

    assert_eq!(begins, 1);
    assert_eq!(commits, 1);
    assert_eq!(
        db.get_node_property(node_id, "name").unwrap(),
        Value::String("alice".into())
    );
    assert_eq!(db.get_edge(edge_id).unwrap().src, node_id);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn explicit_transaction_rollback_restores_before_images() {
    let dir = temp_dir("explicit_transaction_rollback");
    let mut db = HiveDb::open(&dir).unwrap();

    let node_id = db.create_node().unwrap();
    db.set_node_property(node_id, "name", &Value::String("before".into()))
        .unwrap();

    {
        let mut tx = db.begin().unwrap();
        tx.set_node_property(node_id, "name", &Value::String("after".into()))
            .unwrap();
        let _rolled_back_node = tx.create_node().unwrap();
        tx.rollback().unwrap();
    }

    assert_eq!(
        db.get_node_property(node_id, "name").unwrap(),
        Value::String("before".into())
    );
    assert!(db.get_node(crate::types::pack_record_id(1, 1)).is_err());

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn automatic_checkpoint_truncates_wal_after_interval() {
    let dir = temp_dir("automatic_checkpoint");
    let mut db = HiveDb::open(&dir).unwrap();
    db.set_auto_checkpoint_interval(1);

    let _ = db.create_node().unwrap();

    let mut wal = Wal::open(&dir.join("wal.hive")).unwrap();
    let entries = wal.read_all().unwrap();
    assert!(
        entries.is_empty(),
        "WAL should be truncated by automatic checkpoint"
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn rollback_reuses_newly_allocated_edge_page() {
    let dir = temp_dir("rollback_reuses_edge_page");
    let mut db = HiveDb::open(&dir).unwrap();

    let src = db.create_node().unwrap();
    let dst = db.create_node().unwrap();

    let rolled_back_edge;
    {
        let mut tx = db.begin().unwrap();
        rolled_back_edge = tx.create_edge(src, dst).unwrap();
        tx.rollback().unwrap();
    }

    let committed_edge = db.create_edge(src, dst).unwrap();
    assert_eq!(committed_edge, rolled_back_edge);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn rollback_reuses_newly_allocated_overflow_page() {
    let dir = temp_dir("rollback_reuses_overflow_page");
    let mut db = HiveDb::open(&dir).unwrap();
    let node_id = db.create_node().unwrap();

    let long_value = Value::String("this string must live in overflow".into());
    let rolled_back_overflow_page;
    {
        let mut tx = db.begin().unwrap();
        tx.set_node_property(node_id, "name", &long_value).unwrap();
        let node = tx.get_node(node_id).unwrap();
        rolled_back_overflow_page = node.properties[0].long_value_offset;
        tx.rollback().unwrap();
    }

    assert!(db.get_node_property(node_id, "name").is_err());

    db.set_node_property(node_id, "name", &long_value).unwrap();
    let node = db.get_node(node_id).unwrap();
    assert_eq!(
        node.properties[0].long_value_offset,
        rolled_back_overflow_page
    );

    db.close();
    cleanup_dir(&dir);
}
