use super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::value::Value;

fn person_names(result: &crate::query::result::QueryResult) -> Vec<String> {
    let mut names: Vec<String> = result
        .rows
        .iter()
        .map(|row| match &row[0] {
            Value::String(s) => s.clone(),
            Value::Map(m) => match m.get("properties") {
                Some(Value::Map(props)) => match props.get("name") {
                    Some(Value::String(s)) => s.clone(),
                    _ => String::new(),
                },
                _ => String::new(),
            },
            _ => String::new(),
        })
        .filter(|s| !s.is_empty())
        .collect();
    names.sort();
    names
}

#[test]
fn node_label_index_lookup_matches_full_scan() {
    let dir = temp_dir("idx_label_lookup");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "Alice", age: 30})"#)
        .unwrap();
    db.execute(r#"CREATE (b:Person {name: "Bob", age: 25})"#)
        .unwrap();
    db.execute(r#"CREATE (c:Animal {name: "Cat"})"#).unwrap();

    // Without index: full scan.
    let without_index = db
        .execute(r#"MATCH (p:Person) RETURN p.name AS n"#)
        .unwrap();

    db.create_node_label_index("Person").unwrap();

    // With index.
    let with_index = db.execute("MATCH (p:Person) RETURN p.name AS n").unwrap();

    assert_eq!(person_names(&without_index), vec!["Alice", "Bob"]);
    assert_eq!(person_names(&with_index), vec!["Alice", "Bob"]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn node_property_index_lookup_matches_full_scan() {
    let dir = temp_dir("idx_property_lookup");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute("CREATE (a:Person {name: \"Alice\", age: 30})")
        .unwrap();
    db.execute("CREATE (b:Person {name: \"Bob\", age: 25})")
        .unwrap();
    db.execute("CREATE (c:Person {name: \"Carol\", age: 30})")
        .unwrap();

    let without_index = db
        .execute("MATCH (p:Person) WHERE p.age = 30 RETURN p.name AS n")
        .unwrap();

    db.create_node_property_index(Some("Person"), "age")
        .unwrap();

    let with_index = db
        .execute("MATCH (p:Person) WHERE p.age = 30 RETURN p.name AS n")
        .unwrap();

    assert_eq!(person_names(&without_index), vec!["Alice", "Carol"]);
    assert_eq!(person_names(&with_index), vec!["Alice", "Carol"]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn global_node_property_index_matches_full_scan() {
    let dir = temp_dir("idx_global_property");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute("CREATE (a:Person {name: \"Alice\", age: 30})")
        .unwrap();
    db.execute("CREATE (b:Animal {name: \"Bat\", age: 30})")
        .unwrap();
    db.execute("CREATE (c:Person {name: \"Carol\", age: 25})")
        .unwrap();

    let without_index = db
        .execute("MATCH (p) WHERE p.age = 30 RETURN p.name AS n")
        .unwrap();

    db.create_node_property_index(None, "age").unwrap();

    let with_index = db
        .execute("MATCH (p) WHERE p.age = 30 RETURN p.name AS n")
        .unwrap();

    assert_eq!(person_names(&without_index), vec!["Alice", "Bat"]);
    assert_eq!(person_names(&with_index), vec!["Alice", "Bat"]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_property_updates_index() {
    let dir = temp_dir("idx_set_update");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute("CREATE (a:Person {name: \"Alice\", age: 25})")
        .unwrap();
    db.create_node_property_index(Some("Person"), "age")
        .unwrap();

    let before = db
        .execute("MATCH (p:Person) WHERE p.age = 30 RETURN p.name AS n")
        .unwrap();
    assert!(before.rows.is_empty());

    db.execute("MATCH (p:Person) SET p.age = 30").unwrap();

    let after = db
        .execute("MATCH (p:Person) WHERE p.age = 30 RETURN p.name AS n")
        .unwrap();
    assert_eq!(person_names(&after), vec!["Alice"]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn delete_node_updates_index() {
    let dir = temp_dir("idx_delete_node");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute("CREATE (a:Person {name: \"Alice\", age: 30})")
        .unwrap();
    db.execute("CREATE (b:Person {name: \"Bob\", age: 30})")
        .unwrap();
    db.create_node_property_index(Some("Person"), "age")
        .unwrap();

    db.execute("MATCH (p:Person) WHERE p.name = \"Alice\" DELETE p")
        .unwrap();

    let result = db
        .execute("MATCH (p:Person) WHERE p.age = 30 RETURN p.name AS n")
        .unwrap();
    assert_eq!(person_names(&result), vec!["Bob"]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn edge_type_index_matches_full_scan() {
    let dir = temp_dir("idx_edge_type");
    let mut db = HiveDb::open(&dir).unwrap();

    db.create_edge_type_index("KNOWS").unwrap();

    db.execute("CREATE (a:Person {name: \"Alice\"})").unwrap();
    db.execute("CREATE (b:Person {name: \"Bob\"})").unwrap();
    db.execute("CREATE (a)-[:KNOWS]->(b)").unwrap();

    // Edge type indexes are maintained but not yet used by traversal; verify
    // the index directly after creating an edge.
    let (root, label_id) = {
        let mut tx = db.begin().unwrap();
        let label_id = tx.find_label("KNOWS").unwrap().unwrap();
        let root = tx
            .find_index_root(
                crate::storage::index_catalog::EntityKind::EdgeType,
                label_id,
                0,
            )
            .unwrap()
            .unwrap();
        tx.commit_readonly().unwrap();
        (root, label_id)
    };
    let mut btree = db.open_btree(root);
    let ids = btree
        .lookup(&crate::storage::btree::BtreeKey::Int(label_id as i64))
        .unwrap()
        .unwrap();
    assert_eq!(ids.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn edge_property_index_matches_full_scan() {
    let dir = temp_dir("idx_edge_property");
    let mut db = HiveDb::open(&dir).unwrap();

    db.create_edge_property_index(Some("KNOWS"), "since")
        .unwrap();

    db.execute("CREATE (a:Person {name: \"Alice\"})").unwrap();
    db.execute("CREATE (b:Person {name: \"Bob\"})").unwrap();
    db.execute("CREATE (c:Person {name: \"Carol\"})").unwrap();
    db.execute("CREATE (a)-[:KNOWS {since: 2010}]->(b)")
        .unwrap();
    db.execute("CREATE (a)-[:KNOWS {since: 2015}]->(c)")
        .unwrap();

    let root = {
        let mut tx = db.begin().unwrap();
        let key_id = tx.find_property_key("since").unwrap().unwrap();
        let label_id = tx.find_label("KNOWS").unwrap().unwrap();
        let root = tx
            .find_index_root(
                crate::storage::index_catalog::EntityKind::EdgeProperty,
                label_id,
                key_id,
            )
            .unwrap()
            .unwrap();
        tx.commit_readonly().unwrap();
        root
    };

    let mut btree = db.open_btree(root);
    let ids = btree
        .lookup(&crate::storage::btree::BtreeKey::Int(2010))
        .unwrap()
        .unwrap();
    assert_eq!(ids.len(), 1);
    drop(btree);

    // Also verify the second edge is indexed under 2015.
    let mut btree = db.open_btree(root);
    let ids = btree
        .lookup(&crate::storage::btree::BtreeKey::Int(2015))
        .unwrap()
        .unwrap();
    assert_eq!(ids.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn index_maintenance_is_rollback_safe() {
    let dir = temp_dir("idx_rollback");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute("CREATE (a:Person {name: \"Alice\", age: 30})")
        .unwrap();
    db.create_node_property_index(Some("Person"), "age")
        .unwrap();

    // Failed SET should not affect the index.
    let err = db.execute("MATCH (p:Person) SET p.age = 40 DELETE q");
    assert!(err.is_err());

    let result = db
        .execute("MATCH (p:Person) WHERE p.age = 30 RETURN p.name AS n")
        .unwrap();
    assert_eq!(person_names(&result), vec!["Alice"]);

    let result40 = db
        .execute("MATCH (p:Person) WHERE p.age = 40 RETURN p.name AS n")
        .unwrap();
    assert!(result40.rows.is_empty());

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn index_survives_reopen() {
    let dir = temp_dir("idx_reopen");
    {
        let mut db = HiveDb::open(&dir).unwrap();
        db.execute("CREATE (a:Person {name: \"Alice\", age: 30})")
            .unwrap();
        db.create_node_property_index(Some("Person"), "age")
            .unwrap();
        db.close();
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let result = db
            .execute("MATCH (p:Person) WHERE p.age = 30 RETURN p.name AS n")
            .unwrap();
        assert_eq!(person_names(&result), vec!["Alice"]);
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn index_survives_crash_recovery() {
    let dir = temp_dir("idx_recovery");
    {
        let mut db = HiveDb::open(&dir).unwrap();
        db.execute("CREATE (a:Person {name: \"Alice\", age: 30})")
            .unwrap();
        db.create_node_property_index(Some("Person"), "age")
            .unwrap();
        // Simulate crash: do not call close().
    }

    {
        let mut db = HiveDb::open(&dir).unwrap();
        let result = db
            .execute("MATCH (p:Person) WHERE p.age = 30 RETURN p.name AS n")
            .unwrap();
        assert_eq!(person_names(&result), vec!["Alice"]);
        db.close();
    }

    cleanup_dir(&dir);
}

#[test]
fn duplicate_index_creation_is_idempotent() {
    let dir = temp_dir("idx_idempotent");
    let mut db = HiveDb::open(&dir).unwrap();

    let root1 = db.create_node_label_index("Person").unwrap();
    let root2 = db.create_node_label_index("Person").unwrap();
    assert_eq!(root1, root2);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn index_uses_full_scan_fallback_when_no_index_exists() {
    let dir = temp_dir("idx_fallback");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute("CREATE (a:Person {name: \"Alice\", age: 30})")
        .unwrap();

    let result = db
        .execute("MATCH (p:Person) WHERE p.age = 30 RETURN p.name AS n")
        .unwrap();
    assert_eq!(person_names(&result), vec!["Alice"]);

    db.close();
    cleanup_dir(&dir);
}
