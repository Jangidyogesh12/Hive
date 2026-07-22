use super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::value::Value;

#[test]
fn create_match_where_set_return_pipeline() {
    let dir = temp_dir("query_pipeline");
    let mut db = HiveDb::open(&dir).unwrap();

    let created = db
        .execute(r#"CREATE (a:Person {name: "Alice", age: 30}) RETURN a.name"#)
        .unwrap();
    assert_eq!(created.columns, vec!["a.name"]);
    assert_eq!(created.rows, vec![vec![Value::String("Alice".to_string())]]);

    let result = db
        .execute(r#"MATCH (a:Person) WHERE a.age >= 30 SET a.active = true RETURN a.active"#)
        .unwrap();
    assert_eq!(result.rows, vec![vec![Value::Boolean(true)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn relationship_traversal_returns_edge_and_nodes() {
    let dir = temp_dir("query_relationship");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(
        r#"CREATE (a:Person {name: "Alice"})-[:KNOWS {since: 2020}]->(b:Person {name: "Bob"})"#,
    )
    .unwrap();

    let result = db
        .execute(r#"MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN a.name, r.since, b.name"#)
        .unwrap();
    assert_eq!(
        result.rows,
        vec![vec![
            Value::String("Alice".to_string()),
            Value::Integer(2020),
            Value::String("Bob".to_string())
        ]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn merge_reuses_existing_node() {
    let dir = temp_dir("query_merge");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"MERGE (p:Person {email: "alice@example.com"}) RETURN p.email"#)
        .unwrap();
    db.execute(r#"MERGE (p:Person {email: "alice@example.com"}) RETURN p.email"#)
        .unwrap();

    let result = db.execute(r#"MATCH (p:Person) RETURN p.email"#).unwrap();
    assert_eq!(result.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn delete_edge_then_node() {
    let dir = temp_dir("query_delete");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "Alice"})-[:KNOWS]->(b:Person {name: "Bob"})"#)
        .unwrap();
    db.execute(r#"MATCH (a:Person)-[r:KNOWS]->(b:Person) DELETE r"#)
        .unwrap();
    db.execute(r#"MATCH (a:Person {name: "Alice"}) DELETE a"#)
        .unwrap();

    let result = db.execute(r#"MATCH (a:Person) RETURN a.name"#).unwrap();
    assert_eq!(result.rows, vec![vec![Value::String("Bob".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn order_skip_limit_are_applied_to_return() {
    let dir = temp_dir("query_order_limit");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "Alice", age: 30})"#)
        .unwrap();
    db.execute(r#"CREATE (b:Person {name: "Bob", age: 20})"#)
        .unwrap();
    db.execute(r#"CREATE (c:Person {name: "Carol", age: 40})"#)
        .unwrap();

    let result = db
        .execute(r#"MATCH (p:Person) RETURN p.name ORDER BY p.age DESC SKIP 1 LIMIT 1"#)
        .unwrap();
    assert_eq!(result.rows, vec![vec![Value::String("Alice".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}
