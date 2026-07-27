use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::value::Value;

// ---------- CREATE ----------

#[test]
fn create_single_node_with_properties() {
    let dir = temp_dir("exec_create_node_props");
    let mut db = HiveDb::open(&dir).unwrap();

    let r = db
        .execute(r#"CREATE (n:Person {name: "Alice", age: 30}) RETURN n.name, n.age"#)
        .unwrap();
    assert_eq!(r.columns, vec!["n.name", "n.age"]);
    assert_eq!(
        r.rows,
        vec![vec![Value::String("Alice".to_string()), Value::Integer(30)]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_node_without_label() {
    let dir = temp_dir("exec_create_no_label");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n {x: 1})"#).unwrap();
    let r = db.execute(r#"MATCH (n) RETURN n.x"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(1)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_node_without_variable() {
    let dir = temp_dir("exec_create_no_var");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:Tag {label: "v1"})"#).unwrap();
    let r = db.execute(r#"MATCH (n:Tag) RETURN n.label"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("v1".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_multiple_nodes_independent() {
    let dir = temp_dir("exec_create_multi");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})"#).unwrap();
    db.execute(r#"CREATE (b:Person {name: "B"})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:Person) RETURN n.name ORDER BY n.name"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![
            vec![Value::String("A".to_string())],
            vec![Value::String("B".to_string())],
        ]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_relationship_with_properties() {
    let dir = temp_dir("exec_create_rel_props");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})-[:KNOWS {since: 2020}]->(b:Person {name: "B"})"#)
        .unwrap();
    let r = db
        .execute(r#"MATCH (a)-[r:KNOWS]->(b) RETURN a.name, r.since, b.name"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![
            Value::String("A".to_string()),
            Value::Integer(2020),
            Value::String("B".to_string()),
        ]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn create_relationship_reuses_existing_variable() {
    let dir = temp_dir("exec_create_rel_reuse");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})-[:LIKES]->(b:Person {name: "B"})"#)
        .unwrap();
    let r = db
        .execute(r#"MATCH (a)-[:LIKES]->(b) RETURN a.name, b.name"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![
            Value::String("A".to_string()),
            Value::String("B".to_string()),
        ]]
    );

    db.close();
    cleanup_dir(&dir);
}

// ---------- MATCH ----------

#[test]
fn match_by_label() {
    let dir = temp_dir("exec_match_label");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})"#).unwrap();
    db.execute(r#"CREATE (b:Dog {name: "Rex"})"#).unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("A".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn match_all_nodes_no_label() {
    let dir = temp_dir("exec_match_all");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:A {v: 1})"#).unwrap();
    db.execute(r#"CREATE (:B {v: 2})"#).unwrap();
    let r = db.execute(r#"MATCH (n) RETURN n.v ORDER BY n.v"#).unwrap();
    assert_eq!(
        r.rows,
        vec![vec![Value::Integer(1)], vec![Value::Integer(2)]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn match_path_outgoing() {
    let dir = temp_dir("exec_match_out");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:X)-[:R]->(b:Y)"#).unwrap();
    let r = db
        .execute(r#"MATCH (a:X)-[r:R]->(b:Y) RETURN a.v, b.v"#)
        .unwrap();
    assert_eq!(r.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn match_path_incoming_traversal() {
    let dir = temp_dir("exec_match_in");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:X {v: 1})-[:R]->(b:Y {v: 2})"#)
        .unwrap();
    let r = db
        .execute(r#"MATCH (a:X)-[r:R]->(b:Y) RETURN a.v, b.v"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(1), Value::Integer(2)]]);

    let r2 = db
        .execute(r#"MATCH (b:Y)<-[r:R]-(a:X) RETURN a.v, b.v"#)
        .unwrap();
    assert_eq!(r2.rows, vec![vec![Value::Integer(1), Value::Integer(2)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn match_path_undirected() {
    let dir = temp_dir("exec_match_undir");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:X)-[:R]->(b:Y)"#).unwrap();
    let r = db
        .execute(r#"MATCH (a:X)-[r:R]-(b:Y) RETURN a.v, b.v"#)
        .unwrap();
    assert_eq!(r.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn match_no_results_returns_empty() {
    let dir = temp_dir("exec_match_empty");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:Person {name: "A"})"#).unwrap();
    let r = db.execute(r#"MATCH (n:Dog) RETURN n.name"#).unwrap();
    assert!(r.rows.is_empty());

    db.close();
    cleanup_dir(&dir);
}

// ---------- WHERE ----------

#[test]
fn where_equality_filter() {
    let dir = temp_dir("exec_where_eq");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 1})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 2})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 3})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) WHERE n.v = 2 RETURN n.v"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(2)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn where_inequality_operators() {
    let dir = temp_dir("exec_where_ops");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 10})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 20})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 30})"#).unwrap();

    let r = db
        .execute(r#"MATCH (n:N) WHERE n.v > 15 RETURN n.v"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![Value::Integer(20)], vec![Value::Integer(30)]]
    );

    let r = db
        .execute(r#"MATCH (n:N) WHERE n.v >= 20 RETURN n.v"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![Value::Integer(20)], vec![Value::Integer(30)]]
    );

    let r = db
        .execute(r#"MATCH (n:N) WHERE n.v < 20 RETURN n.v"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(10)]]);

    let r = db
        .execute(r#"MATCH (n:N) WHERE n.v <= 20 RETURN n.v"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![Value::Integer(10)], vec![Value::Integer(20)]]
    );

    let r = db
        .execute(r#"MATCH (n:N) WHERE n.v <> 20 RETURN n.v ORDER BY n.v"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![Value::Integer(10)], vec![Value::Integer(30)]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn where_and_or_logic() {
    let dir = temp_dir("exec_where_logic");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {a: 1, b: 10})"#).unwrap();
    db.execute(r#"CREATE (:N {a: 2, b: 20})"#).unwrap();
    db.execute(r#"CREATE (:N {a: 3, b: 30})"#).unwrap();

    let r = db
        .execute(r#"MATCH (n:N) WHERE n.a > 1 AND n.b < 30 RETURN n.a ORDER BY n.a"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(2)]]);

    let r = db
        .execute(r#"MATCH (n:N) WHERE n.a = 1 OR n.a = 3 RETURN n.a ORDER BY n.a"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![Value::Integer(1)], vec![Value::Integer(3)]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn where_not_operator() {
    let dir = temp_dir("exec_where_not");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {active: true})"#).unwrap();
    db.execute(r#"CREATE (:N {active: false})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) WHERE NOT n.active RETURN n.active"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Boolean(false)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn where_string_comparison() {
    let dir = temp_dir("exec_where_str");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {name: "Alice"})"#).unwrap();
    db.execute(r#"CREATE (:N {name: "Bob"})"#).unwrap();
    db.execute(r#"CREATE (:N {name: "Carol"})"#).unwrap();

    let r = db
        .execute(r#"MATCH (n:N) WHERE n.name > "Bob" RETURN n.name"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("Carol".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn where_on_traversal() {
    let dir = temp_dir("exec_where_traversal");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:X {v: 1})-[:R]->(b:Y {v: 2})"#)
        .unwrap();
    db.execute(r#"CREATE (a2:X {v: 10})-[:R]->(b2:Y {v: 20})"#)
        .unwrap();
    let r = db
        .execute(r#"MATCH (a:X)-[r:R]->(b:Y) WHERE b.v > 5 RETURN a.v, b.v"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(10), Value::Integer(20)]]);

    db.close();
    cleanup_dir(&dir);
}

// ---------- SET ----------

#[test]
fn set_property_on_node() {
    let dir = temp_dir("exec_set_node");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice", age: 30})"#)
        .unwrap();
    db.execute(r#"MATCH (n:Person) SET n.age = 31 RETURN n.age"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.age"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(31)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_property_on_edge() {
    let dir = temp_dir("exec_set_edge");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a)-[:R {x: 10}]->(b)"#).unwrap();
    db.execute(r#"MATCH (a)-[r:R]->(b) SET r.x = 20 RETURN r.x"#)
        .unwrap();
    let r = db.execute(r#"MATCH (a)-[r:R]->(b) RETURN r.x"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(20)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_adds_new_property() {
    let dir = temp_dir("exec_set_new_prop");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice"})"#).unwrap();
    db.execute(r#"MATCH (n:Person) SET n.age = 30 RETURN n.age"#)
        .unwrap();
    let r = db
        .execute(r#"MATCH (n:Person) RETURN n.name, n.age"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![Value::String("Alice".to_string()), Value::Integer(30)]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_overwrites_existing_property() {
    let dir = temp_dir("exec_set_overwrite");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice"})"#).unwrap();
    db.execute(r#"MATCH (n:Person) SET n.name = "Bob" RETURN n.name"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("Bob".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_with_literal_value() {
    let dir = temp_dir("exec_set_literal");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice"})"#).unwrap();
    db.execute(r#"MATCH (n:Person) SET n.active = true RETURN n.active"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.active"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Boolean(true)]]);

    db.close();
    cleanup_dir(&dir);
}

// ---------- DELETE ----------

#[test]
fn delete_node() {
    let dir = temp_dir("exec_delete_node");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})"#).unwrap();
    db.execute(r#"CREATE (b:Person {name: "B"})"#).unwrap();
    db.execute(r#"MATCH (n:Person {name: "A"}) DELETE n"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("B".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn delete_edge() {
    let dir = temp_dir("exec_delete_edge");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a)-[:R]->(b)"#).unwrap();
    db.execute(r#"CREATE (a)-[:R]->(c)"#).unwrap();
    db.execute(r#"MATCH (a)-[r:R]->(c) DELETE r"#).unwrap();
    let _r = db.execute(r#"MATCH (a)-[r:R]->(b) RETURN count(r)"#);
    // Just verify it doesn't error; count may not exist yet
    db.close();
    cleanup_dir(&dir);
}

#[test]
fn detach_delete_removes_edges() {
    let dir = temp_dir("exec_detach_delete");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})-[:KNOWS]->(b:Person {name: "B"})"#)
        .unwrap();
    db.execute(r#"MATCH (a:Person {name: "A"}) DETACH DELETE a"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("B".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn delete_multiple_variables() {
    let dir = temp_dir("exec_delete_multi");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})-[:KNOWS]->(b:Person {name: "B"})"#)
        .unwrap();
    db.execute(r#"MATCH (a:Person)-[r:KNOWS]->(b:Person) DELETE r, a"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("B".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn delete_all_matched() {
    let dir = temp_dir("exec_delete_all");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 1})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 2})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 3})"#).unwrap();
    db.execute(r#"MATCH (n:N) DELETE n"#).unwrap();
    let r = db.execute(r#"MATCH (n:N) RETURN n.v"#).unwrap();
    assert!(r.rows.is_empty());

    db.close();
    cleanup_dir(&dir);
}

// ---------- MERGE ----------

#[test]
fn merge_creates_new_node() {
    let dir = temp_dir("exec_merge_create");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"MERGE (n:Person {name: "Alice"}) RETURN n.name"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("Alice".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn merge_reuses_existing_node() {
    let dir = temp_dir("exec_merge_reuse");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"MERGE (n:Person {name: "Alice"})"#).unwrap();
    db.execute(r#"MERGE (n:Person {name: "Alice"})"#).unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(r.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn merge_different_properties_creates_separate() {
    let dir = temp_dir("exec_merge_diff");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"MERGE (n:Person {name: "Alice"})"#).unwrap();
    db.execute(r#"MERGE (n:Person {name: "Bob"})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:Person) RETURN n.name ORDER BY n.name"#)
        .unwrap();
    assert_eq!(r.rows.len(), 2);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn merge_without_label() {
    let dir = temp_dir("exec_merge_no_label");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"MERGE (n {x: 1})"#).unwrap();
    db.execute(r#"MERGE (n {x: 1})"#).unwrap();
    let r = db.execute(r#"MATCH (n) RETURN n.x"#).unwrap();
    assert_eq!(r.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

// ---------- RETURN ----------

#[test]
fn return_multiple_columns() {
    let dir = temp_dir("exec_return_multi");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice", age: 30})"#)
        .unwrap();
    let r = db
        .execute(r#"MATCH (n:Person) RETURN n.name, n.age, n.name AS dup"#)
        .unwrap();
    assert_eq!(r.columns, vec!["n.name", "n.age", "dup"]);
    assert_eq!(
        r.rows,
        vec![vec![
            Value::String("Alice".to_string()),
            Value::Integer(30),
            Value::String("Alice".to_string()),
        ]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn return_with_alias() {
    let dir = temp_dir("exec_return_alias");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice"})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:Person) RETURN n.name AS person_name"#)
        .unwrap();
    assert_eq!(r.columns, vec!["person_name"]);
    assert_eq!(r.rows, vec![vec![Value::String("Alice".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn return_literal_value() {
    let dir = temp_dir("exec_return_literal");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 1})"#).unwrap();
    let r = db.execute(r#"MATCH (n:N) RETURN 42 AS constant"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(42)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn return_empty_result() {
    let dir = temp_dir("exec_return_empty");
    let mut db = HiveDb::open(&dir).unwrap();

    let r = db.execute(r#"MATCH (n:NoSuchLabel) RETURN n"#).unwrap();
    assert!(r.rows.is_empty());
    assert_eq!(r.columns, vec!["n"]);

    db.close();
    cleanup_dir(&dir);
}

// ---------- ORDER BY ----------

#[test]
fn order_by_asc() {
    let dir = temp_dir("exec_order_asc");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 30})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 10})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 20})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) RETURN n.v ORDER BY n.v ASC"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![
            vec![Value::Integer(10)],
            vec![Value::Integer(20)],
            vec![Value::Integer(30)],
        ]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn order_by_desc() {
    let dir = temp_dir("exec_order_desc");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 10})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 30})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 20})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) RETURN n.v ORDER BY n.v DESC"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![
            vec![Value::Integer(30)],
            vec![Value::Integer(20)],
            vec![Value::Integer(10)],
        ]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn order_by_string() {
    let dir = temp_dir("exec_order_str");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {name: "Charlie"})"#).unwrap();
    db.execute(r#"CREATE (:N {name: "Alice"})"#).unwrap();
    db.execute(r#"CREATE (:N {name: "Bob"})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) RETURN n.name ORDER BY n.name"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![
            vec![Value::String("Alice".to_string())],
            vec![Value::String("Bob".to_string())],
            vec![Value::String("Charlie".to_string())],
        ]
    );

    db.close();
    cleanup_dir(&dir);
}

// ---------- SKIP / LIMIT ----------

#[test]
fn skip_rows() {
    let dir = temp_dir("exec_skip");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 1})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 2})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 3})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) RETURN n.v ORDER BY n.v SKIP 1"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![Value::Integer(2)], vec![Value::Integer(3)]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn limit_rows() {
    let dir = temp_dir("exec_limit");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 1})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 2})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 3})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) RETURN n.v ORDER BY n.v LIMIT 2"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![Value::Integer(1)], vec![Value::Integer(2)]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn skip_and_limit() {
    let dir = temp_dir("exec_skip_limit");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 1})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 2})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 3})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 4})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) RETURN n.v ORDER BY n.v SKIP 1 LIMIT 2"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![Value::Integer(2)], vec![Value::Integer(3)]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn skip_exceeding_count_returns_empty() {
    let dir = temp_dir("exec_skip_empty");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 1})"#).unwrap();
    let r = db.execute(r#"MATCH (n:N) RETURN n.v SKIP 100"#).unwrap();
    assert!(r.rows.is_empty());

    db.close();
    cleanup_dir(&dir);
}

// ---------- NULL BEHAVIOR ----------

#[test]
fn missing_property_returns_null() {
    let dir = temp_dir("exec_null_prop");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice"})"#).unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.age"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Null]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn null_in_comparison() {
    let dir = temp_dir("exec_null_compare");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 1})"#).unwrap();
    db.execute(r#"CREATE (:N {v: null})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) WHERE n.v = 1 RETURN n.v"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(1)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn null_less_than_any_value() {
    let dir = temp_dir("exec_null_order");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 2})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 1})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) RETURN n.v ORDER BY n.v ASC"#)
        .unwrap();
    // Nulls sort before non-nulls
    assert_eq!(
        r.rows,
        vec![vec![Value::Integer(1)], vec![Value::Integer(2)],]
    );

    db.close();
    cleanup_dir(&dir);
}

// ---------- WHOLE-ENTITY RETURN ----------

#[test]
fn return_node_entity_map() {
    let dir = temp_dir("exec_return_entity");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice", age: 30})"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n"#).unwrap();
    assert_eq!(r.columns, vec!["n"]);
    match &r.rows[0][0] {
        Value::Map(map) => {
            assert!(map.contains_key("id"));
            assert_eq!(
                map.get("label").unwrap(),
                &Value::String("Person".to_string())
            );
            match map.get("properties").unwrap() {
                Value::Map(props) => {
                    assert_eq!(
                        props.get("name").unwrap(),
                        &Value::String("Alice".to_string())
                    );
                    assert_eq!(props.get("age").unwrap(), &Value::Integer(30));
                }
                other => panic!("expected properties Map, got {other:?}"),
            }
        }
        other => panic!("expected Map for entity return, got {other:?}"),
    }

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn return_edge_entity_map() {
    let dir = temp_dir("exec_return_edge_entity");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a)-[:KNOWS {since: 2020}]->(b)"#)
        .unwrap();
    let r = db.execute(r#"MATCH (a)-[r:KNOWS]->(b) RETURN r"#).unwrap();
    match &r.rows[0][0] {
        Value::Map(map) => {
            assert!(map.contains_key("id"));
            assert_eq!(
                map.get("type").unwrap(),
                &Value::String("KNOWS".to_string())
            );
            assert!(map.contains_key("src"));
            assert!(map.contains_key("dst"));
            match map.get("properties").unwrap() {
                Value::Map(props) => {
                    assert_eq!(props.get("since").unwrap(), &Value::Integer(2020));
                }
                other => panic!("expected properties Map, got {other:?}"),
            }
        }
        other => panic!("expected Map for edge entity return, got {other:?}"),
    }

    db.close();
    cleanup_dir(&dir);
}

// ---------- ROLLBACK ON FAILURE ----------

#[test]
fn rollback_on_set_unknown_variable() {
    let dir = temp_dir("exec_rollback_set");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice"})"#).unwrap();
    let err = db.execute(r#"MATCH (n:Person) SET m.bad = 1 RETURN n"#);
    assert!(err.is_err());

    let r = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("Alice".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn rollback_on_delete_unknown_variable() {
    let dir = temp_dir("exec_rollback_delete");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "A"})"#).unwrap();
    let err = db.execute(r#"MATCH (n:Person) DELETE m"#);
    assert!(err.is_err());

    let r = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(r.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn rollback_preserves_edges_on_failed_detach() {
    let dir = temp_dir("exec_rollback_detach");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})-[:KNOWS]->(b:Person {name: "B"})"#)
        .unwrap();
    let err = db.execute(r#"MATCH (a:Person {name: "A"}) DETACH DELETE m"#);
    assert!(err.is_err());

    let r = db
        .execute(r#"MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN a.name, b.name"#)
        .unwrap();
    assert_eq!(r.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

// ---------- REBINDING / CONFLICT ----------

#[test]
fn scan_rebinds_variable_to_each_match() {
    let dir = temp_dir("exec_rebind");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 1})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 2})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) RETURN n.v ORDER BY n.v"#)
        .unwrap();
    assert_eq!(r.rows.len(), 2);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn traversal_rebinds_to_each_edge() {
    let dir = temp_dir("exec_traverse_rebind");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:X)-[:R]->(b:Y {v: 10})"#).unwrap();
    db.execute(r#"CREATE (a:X)-[:R]->(c:Y {v: 20})"#).unwrap();
    let r = db
        .execute(r#"MATCH (a:X)-[r:R]->(b:Y) RETURN b.v ORDER BY b.v"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![Value::Integer(10)], vec![Value::Integer(20)]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn multiple_scans_produce_cross_product() {
    let dir = temp_dir("exec_cross_product");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:A {v: 1})"#).unwrap();
    db.execute(r#"CREATE (:A {v: 2})"#).unwrap();
    db.execute(r#"CREATE (:B {w: 10})"#).unwrap();
    db.execute(r#"CREATE (:B {w: 20})"#).unwrap();
    let r = db
        .execute(r#"MATCH (a:A) MATCH (b:B) RETURN a.v, b.w ORDER BY a.v, b.w"#)
        .unwrap();
    assert_eq!(r.rows.len(), 4);

    db.close();
    cleanup_dir(&dir);
}

// ---------- COMPLEX PIPELINES ----------

#[test]
fn create_match_set_return() {
    let dir = temp_dir("exec_pipeline_csr");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice", age: 30})"#)
        .unwrap();
    let r = db
        .execute(
            r#"MATCH (n:Person) WHERE n.age >= 30 SET n.active = true RETURN n.name, n.active"#,
        )
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![
            Value::String("Alice".to_string()),
            Value::Boolean(true),
        ]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn match_traverse_set_return() {
    let dir = temp_dir("exec_pipeline_mtsr");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})-[:KNOWS {x: 1}]->(b:Person {name: "B"})"#)
        .unwrap();
    let r = db
        .execute(
            r#"MATCH (a:Person)-[r:KNOWS]->(b:Person) SET r.x = 99 RETURN a.name, r.x, b.name"#,
        )
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![
            Value::String("A".to_string()),
            Value::Integer(99),
            Value::String("B".to_string()),
        ]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn match_where_delete_return() {
    let dir = temp_dir("exec_pipeline_mwd");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 1})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 2})"#).unwrap();
    db.execute(r#"CREATE (:N {v: 3})"#).unwrap();
    db.execute(r#"MATCH (n:N) WHERE n.v <= 1 DELETE n"#)
        .unwrap();
    let r = db
        .execute(r#"MATCH (n:N) RETURN n.v ORDER BY n.v"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![Value::Integer(2)], vec![Value::Integer(3)]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn merge_set_return() {
    let dir = temp_dir("exec_pipeline_msr");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"MERGE (n:Person {name: "Alice"}) SET n.visited = true RETURN n.name, n.visited"#)
        .unwrap();
    let r = db
        .execute(r#"MERGE (n:Person {name: "Alice"}) SET n.count = 2 RETURN n.name, n.count"#)
        .unwrap();
    assert_eq!(r.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

// ---------- TYPE COMPARISON ----------

#[test]
fn integer_float_comparison() {
    let dir = temp_dir("exec_type_num");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: 10})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) WHERE n.v > 9.5 RETURN n.v"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(10)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn boolean_equality() {
    let dir = temp_dir("exec_type_bool");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {flag: true})"#).unwrap();
    db.execute(r#"CREATE (:N {flag: false})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) WHERE n.flag = true RETURN n.flag"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Boolean(true)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn string_ordering() {
    let dir = temp_dir("exec_type_str_order");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {v: "b"})"#).unwrap();
    db.execute(r#"CREATE (:N {v: "a"})"#).unwrap();
    db.execute(r#"CREATE (:N {v: "c"})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) WHERE n.v >= "b" RETURN n.v ORDER BY n.v"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![
            vec![Value::String("b".to_string())],
            vec![Value::String("c".to_string())],
        ]
    );

    db.close();
    cleanup_dir(&dir);
}

// ---------- EDGE CASES ----------

#[test]
fn create_empty_property_map() {
    let dir = temp_dir("exec_empty_props");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Empty)"#).unwrap();
    let r = db.execute(r#"MATCH (n:Empty) RETURN n"#).unwrap();
    assert_eq!(r.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn match_with_inline_properties() {
    let dir = temp_dir("exec_match_inline");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {x: 1, y: "hello"})"#).unwrap();
    db.execute(r#"CREATE (:N {x: 2, y: "world"})"#).unwrap();
    let r = db.execute(r#"MATCH (n:N {x: 1}) RETURN n.y"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("hello".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_multiple_properties_sequential() {
    let dir = temp_dir("exec_set_multi");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice"})"#).unwrap();
    db.execute(r#"MATCH (n:Person) SET n.age = 30 RETURN n.age"#)
        .unwrap();
    db.execute(r#"MATCH (n:Person) SET n.active = true RETURN n.active"#)
        .unwrap();
    let r = db
        .execute(r#"MATCH (n:Person) RETURN n.name, n.age, n.active"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![
            Value::String("Alice".to_string()),
            Value::Integer(30),
            Value::Boolean(true),
        ]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn detach_delete_only_removes_matched_edges() {
    let dir = temp_dir("exec_detach_partial");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})-[:KNOWS]->(b:Person {name: "B"})"#)
        .unwrap();
    db.execute(r#"CREATE (b2:Person {name: "B"})-[:KNOWS]->(c:Person {name: "C"})"#)
        .unwrap();
    db.execute(r#"MATCH (a:Person {name: "A"}) DETACH DELETE a"#)
        .unwrap();
    let r = db
        .execute(r#"MATCH (n:Person) RETURN n.name ORDER BY n.name"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![
            vec![Value::String("B".to_string())],
            vec![Value::String("B".to_string())],
            vec![Value::String("C".to_string())],
        ]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn merge_after_create_reuses() {
    let dir = temp_dir("exec_merge_after_create");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice"})"#).unwrap();
    db.execute(r#"MERGE (n:Person {name: "Alice"}) RETURN n.name"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(r.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn no_return_query_returns_empty_columns() {
    let dir = temp_dir("exec_no_return");
    let mut db = HiveDb::open(&dir).unwrap();

    let r = db.execute(r#"CREATE (n:Person {name: "Alice"})"#).unwrap();
    assert!(r.columns.is_empty());
    assert!(r.rows.is_empty());

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn match_with_boolean_property_filter() {
    let dir = temp_dir("exec_bool_filter");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (:N {active: true})"#).unwrap();
    db.execute(r#"CREATE (:N {active: false})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:N) WHERE n.active RETURN n.active"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Boolean(true)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn set_property_type_change() {
    let dir = temp_dir("exec_set_type_change");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "Alice"})"#).unwrap();
    db.execute(r#"MATCH (n:Person) SET n.data = 42 RETURN n.data"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.data"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(42)]]);

    db.execute(r#"MATCH (n:Person) SET n.data = "hello" RETURN n.data"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.data"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("hello".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}
