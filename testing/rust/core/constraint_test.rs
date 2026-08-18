use super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;

fn count_nodes(db: &mut HiveDb) -> usize {
    db.execute("MATCH (n) RETURN n").unwrap().rows.len()
}

#[test]
fn unique_constraint_blocks_duplicate_create() {
    let dir = temp_dir("uc_dup_create");
    let mut db = HiveDb::open(&dir).unwrap();

    db.create_unique_constraint("Person", "email").unwrap();

    db.execute(r#"CREATE (a:Person {email: "alice@example.com"})"#)
        .unwrap();
    let err = db.execute(r#"CREATE (b:Person {email: "alice@example.com"})"#);
    assert!(err.is_err(), "duplicate value should be rejected");

    assert_eq!(count_nodes(&mut db), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn unique_constraint_allows_different_values() {
    let dir = temp_dir("uc_diff_values");
    let mut db = HiveDb::open(&dir).unwrap();

    db.create_unique_constraint("Person", "email").unwrap();

    db.execute(r#"CREATE (a:Person {email: "alice@example.com"})"#)
        .unwrap();
    db.execute(r#"CREATE (b:Person {email: "bob@example.com"})"#)
        .unwrap();

    assert_eq!(count_nodes(&mut db), 2);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn unique_constraint_is_per_label() {
    let dir = temp_dir("uc_per_label");
    let mut db = HiveDb::open(&dir).unwrap();

    db.create_unique_constraint("Person", "name").unwrap();

    db.execute(r#"CREATE (a:Person {name: "Alice"})"#).unwrap();
    db.execute(r#"CREATE (b:Animal {name: "Alice"})"#).unwrap();

    assert_eq!(count_nodes(&mut db), 2);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn unique_constraint_blocks_duplicate_set() {
    let dir = temp_dir("uc_dup_set");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {email: "alice@example.com"})"#)
        .unwrap();
    db.execute(r#"CREATE (b:Person {email: "bob@example.com"})"#)
        .unwrap();
    db.create_unique_constraint("Person", "email").unwrap();

    let err = db.execute(
        r#"MATCH (b:Person) WHERE b.email = "bob@example.com" SET b.email = "alice@example.com""#,
    );
    assert!(err.is_err(), "SET to duplicate value should be rejected");

    let emails: Vec<String> = db
        .execute(r#"MATCH (p:Person) RETURN p.email AS e"#)
        .unwrap()
        .rows
        .iter()
        .map(|row| match &row[0] {
            crate::value::Value::String(s) => s.clone(),
            _ => String::new(),
        })
        .collect();
    assert!(emails.contains(&"alice@example.com".to_string()));
    assert!(emails.contains(&"bob@example.com".to_string()));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn unique_constraint_allows_idempotent_set() {
    let dir = temp_dir("uc_idempotent_set");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {email: "alice@example.com"})"#)
        .unwrap();
    db.create_unique_constraint("Person", "email").unwrap();

    db.execute(r#"MATCH (a:Person) SET a.email = "alice@example.com""#)
        .unwrap();

    assert_eq!(count_nodes(&mut db), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn unique_constraint_merge_is_idempotent() {
    let dir = temp_dir("uc_merge_idempotent");
    let mut db = HiveDb::open(&dir).unwrap();

    db.create_unique_constraint("Person", "email").unwrap();

    db.execute(r#"MERGE (a:Person {email: "alice@example.com"})"#)
        .unwrap();
    db.execute(r#"MERGE (b:Person {email: "alice@example.com"})"#)
        .unwrap();

    assert_eq!(count_nodes(&mut db), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn unique_constraint_rollback_leaves_no_data() {
    let dir = temp_dir("uc_rollback");
    let mut db = HiveDb::open(&dir).unwrap();

    db.create_unique_constraint("Person", "email").unwrap();

    db.execute(r#"CREATE (a:Person {email: "alice@example.com"})"#)
        .unwrap();

    // Failed create should not leave a node behind.
    let _ = db.execute(r#"CREATE (b:Person {email: "alice@example.com"})"#);

    assert_eq!(count_nodes(&mut db), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn unique_constraint_survives_reopen() {
    let dir = temp_dir("uc_reopen");
    {
        let mut db = HiveDb::open(&dir).unwrap();
        db.create_unique_constraint("Person", "email").unwrap();
        db.execute(r#"CREATE (a:Person {email: "alice@example.com"})"#)
            .unwrap();
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let err = db.execute(r#"CREATE (b:Person {email: "alice@example.com"})"#);
        assert!(err.is_err());
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn unique_constraint_survives_crash_recovery() {
    let dir = temp_dir("uc_recovery");
    {
        let mut db = HiveDb::open(&dir).unwrap();
        db.create_unique_constraint("Person", "email").unwrap();
        db.execute(r#"CREATE (a:Person {email: "alice@example.com"})"#)
            .unwrap();
        // Simulate crash.
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let err = db.execute(r#"CREATE (b:Person {email: "alice@example.com"})"#);
        assert!(err.is_err());
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn unique_constraint_creation_is_idempotent() {
    let dir = temp_dir("uc_idempotent_create");
    let mut db = HiveDb::open(&dir).unwrap();

    db.create_unique_constraint("Person", "email").unwrap();
    db.create_unique_constraint("Person", "email").unwrap();

    db.execute(r#"CREATE (a:Person {email: "alice@example.com"})"#)
        .unwrap();
    let err = db.execute(r#"CREATE (b:Person {email: "alice@example.com"})"#);
    assert!(err.is_err());

    db.close();
    cleanup_dir(&dir);
}
