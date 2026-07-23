use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;

#[test]
fn register_and_get_label() {
    let dir = temp_dir("label_register");
    let mut db = HiveDb::open(&dir).unwrap();

    let person_id = db.register_label("Person").unwrap();
    assert_eq!(person_id, 1);

    let company_id = db.register_label("Company").unwrap();
    assert_eq!(company_id, 2);

    assert_eq!(db.get_label_name(person_id).unwrap(), Some("Person".into()));
    assert_eq!(
        db.get_label_name(company_id).unwrap(),
        Some("Company".into())
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn register_same_label_returns_existing_id() {
    let dir = temp_dir("label_dedup");
    let mut db = HiveDb::open(&dir).unwrap();

    let id1 = db.register_label("Person").unwrap();
    let id2 = db.register_label("Person").unwrap();
    assert_eq!(id1, id2);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_node_with_label() {
    let dir = temp_dir("node_with_label");
    let mut db = HiveDb::open(&dir).unwrap();

    let person_id = db.register_label("Person").unwrap();
    let node = db.create_node_with_label(person_id).unwrap();

    let record = db.get_node(node).unwrap();
    assert_eq!(record.label_id, person_id);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_edge_with_label() {
    let dir = temp_dir("edge_with_label");
    let mut db = HiveDb::open(&dir).unwrap();

    let person_id = db.register_label("Person").unwrap();
    let knows_id = db.register_label("KNOWS").unwrap();

    let alice = db.create_node_with_label(person_id).unwrap();
    let bob = db.create_node_with_label(person_id).unwrap();
    let edge = db.create_edge_with_label(alice, bob, knows_id).unwrap();

    let record = db.get_edge(edge).unwrap();
    assert_eq!(record.label_id, knows_id);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn labels_persist_after_reopen() {
    let dir = temp_dir("label_persist");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let person_id = db.register_label("Person").unwrap();
        let node = db.create_node_with_label(person_id).unwrap();
        let self_id = db.register_label("SELF").unwrap();
        let _edge = db.create_edge_with_label(node, node, self_id).unwrap();
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        assert_eq!(db.get_label_name(1).unwrap(), Some("Person".into()));
        assert_eq!(db.get_label_name(2).unwrap(), Some("SELF".into()));
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn transaction_label_registration_rolls_back() {
    let dir = temp_dir("label_transaction_rollback");
    let mut db = HiveDb::open(&dir).unwrap();

    let label_id;
    {
        let mut tx = db.begin().unwrap();
        label_id = tx.register_label("RolledBack").unwrap();
        tx.rollback().unwrap();
    }

    assert_eq!(db.get_label_name(label_id).unwrap(), None);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn rollback_keeps_existing_label_and_removes_new_label() {
    let dir = temp_dir("label_rollback_existing");
    let mut db = HiveDb::open(&dir).unwrap();

    let person_id = db.register_label("Person").unwrap();
    let company_id;
    {
        let mut tx = db.begin().unwrap();
        assert_eq!(tx.register_label("Person").unwrap(), person_id);
        company_id = tx.register_label("Company").unwrap();
        tx.rollback().unwrap();
    }

    assert_eq!(db.get_label_name(person_id).unwrap(), Some("Person".into()));
    assert_eq!(db.get_label_name(company_id).unwrap(), None);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn transaction_label_registration_commits() {
    let dir = temp_dir("label_transaction_commit");
    let mut db = HiveDb::open(&dir).unwrap();

    let label_id;
    {
        let mut tx = db.begin().unwrap();
        label_id = tx.register_label("Committed").unwrap();
        tx.commit().unwrap();
    }

    assert_eq!(
        db.get_label_name(label_id).unwrap(),
        Some("Committed".into())
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn query_created_label_persists_after_reopen() {
    let dir = temp_dir("query_label_persist");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        db.execute(r#"CREATE (:Person {name: "Alice"})"#).unwrap();
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        assert_eq!(db.get_label_name(1).unwrap(), Some("Person".into()));
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn committed_label_survives_wal_recovery() {
    let dir = temp_dir("label_wal_recovery");

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let label_id = db.register_label("Recovered").unwrap();
        assert_eq!(label_id, 1);
        drop(db);
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        assert_eq!(db.get_label_name(1).unwrap(), Some("Recovered".into()));
        db.close();
    }

    cleanup_dir(&dir);
}
