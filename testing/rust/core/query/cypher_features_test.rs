use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::value::Value;
use std::collections::HashMap;

// ---------- STEP 15: PRODUCTION-SAFE MERGE ----------

#[test]
fn merge_repeated_does_not_duplicate() {
    let dir = temp_dir("feat_merge_nodup");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"MERGE (n:Person {email: "a@b.com"})"#)
        .unwrap();
    db.execute(r#"MERGE (n:Person {email: "a@b.com"})"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.email"#).unwrap();
    assert_eq!(r.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn merge_on_create_set_applies_only_on_create() {
    let dir = temp_dir("feat_merge_oncreate");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"MERGE (n:Person {email: "a@b.com"}) ON CREATE SET n.created = true"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.created"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Boolean(true)]]);

    // Second MERGE matches: ON CREATE must not overwrite.
    db.execute(r#"MERGE (n:Person {email: "a@b.com"}) ON CREATE SET n.created = false"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.created"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Boolean(true)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn merge_on_match_set_applies_only_on_match() {
    let dir = temp_dir("feat_merge_onmatch");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {email: "a@b.com", visits: 1})"#)
        .unwrap();
    db.execute(r#"MERGE (n:Person {email: "a@b.com"}) ON MATCH SET n.visits = 2"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.visits"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(2)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn merge_relationship_is_deterministic() {
    let dir = temp_dir("feat_merge_rel");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})"#).unwrap();
    db.execute(r#"CREATE (b:Person {name: "B"})"#).unwrap();
    db.execute(r#"MERGE (a:Person {name: "A"})-[:KNOWS]->(b:Person {name: "B"})"#)
        .unwrap();
    db.execute(r#"MERGE (a:Person {name: "A"})-[:KNOWS]->(b:Person {name: "B"})"#)
        .unwrap();
    let r = db.execute(r#"MATCH (a)-[r:KNOWS]->(b) RETURN r"#).unwrap();
    assert_eq!(r.rows.len(), 1, "relationship MERGE must not duplicate");

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn merge_relationship_on_create_and_match() {
    let dir = temp_dir("feat_merge_rel_actions");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(
        r#"MERGE (a:Person {name: "A"})-[:KNOWS]->(b:Person {name: "B"}) ON CREATE SET a.first = true"#,
    )
    .unwrap();
    let r = db
        .execute(r#"MATCH (a:Person {name: "A"}) RETURN a.first"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Boolean(true)]]);

    db.execute(
        r#"MERGE (a:Person {name: "A"})-[:KNOWS]->(b:Person {name: "B"}) ON MATCH SET a.seen = true"#,
    )
    .unwrap();
    let r = db
        .execute(r#"MATCH (a:Person {name: "A"}) RETURN a.seen"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Boolean(true)]]);

    db.close();
    cleanup_dir(&dir);
}

// ---------- STEP 16: QUERY PARAMETERS ----------

#[test]
fn params_bind_in_create_match_where_set() {
    let dir = temp_dir("feat_params");
    let mut db = HiveDb::open(&dir).unwrap();

    let mut params = HashMap::new();
    params.insert("name".to_string(), Value::String("Alice".to_string()));
    params.insert("age".to_string(), Value::Integer(30));
    db.execute_with_params(r#"CREATE (n:Person {name: $name, age: $age})"#, &params)
        .unwrap();

    let mut q = HashMap::new();
    q.insert("age".to_string(), Value::Integer(30));
    let r = db
        .execute_with_params(r#"MATCH (n:Person) WHERE n.age = $age RETURN n.name"#, &q)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("Alice".to_string())]]);

    let mut s = HashMap::new();
    s.insert("new_age".to_string(), Value::Integer(31));
    db.execute_with_params(r#"MATCH (n:Person) SET n.age = $new_age"#, &s)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.age"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(31)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn missing_param_returns_error() {
    let dir = temp_dir("feat_params_missing");
    let mut db = HiveDb::open(&dir).unwrap();

    // A row must exist so the `$age` expression is actually evaluated.
    db.execute(r#"CREATE (n:Person {name: "A"})"#).unwrap();
    let err = db
        .execute_with_params(
            r#"MATCH (n:Person) WHERE n.age = $age RETURN n"#,
            &HashMap::new(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("missing query parameter"), "{err}");

    db.close();
    cleanup_dir(&dir);
}

// ---------- STEP 17: WITH PIPELINE ----------

#[test]
fn with_projects_and_limits_scope() {
    let dir = temp_dir("feat_with_scope");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A", age: 1})"#)
        .unwrap();
    db.execute(r#"CREATE (b:Person {name: "B", age: 2})"#)
        .unwrap();

    let r = db
        .execute(r#"MATCH (n:Person) WITH n AS m RETURN m.name ORDER BY m.name"#)
        .unwrap();
    assert_eq!(r.rows.len(), 2);

    // Variables not projected by WITH are not visible later.
    let err = db
        .execute(r#"MATCH (n:Person) WITH n.name AS name RETURN n"#)
        .unwrap_err();
    assert!(err.to_string().contains("unknown variable"), "{err}");

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn with_chains_match() {
    let dir = temp_dir("feat_with_chain");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})-[:KNOWS]->(b:Person {name: "B"})"#)
        .unwrap();
    let r = db
        .execute(r#"MATCH (a:Person) WITH a AS x MATCH (x)-[:KNOWS]->(b) RETURN b.name"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("B".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

// ---------- STEP 18: AGGREGATION AND COUNT ----------

#[test]
fn count_star_counts_rows() {
    let dir = temp_dir("feat_count_star");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person)"#).unwrap();
    db.execute(r#"CREATE (b:Person)"#).unwrap();
    db.execute(r#"CREATE (c:Person)"#).unwrap();

    let r = db.execute(r#"MATCH (n:Person) RETURN COUNT(*)"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(3)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn count_expr_skips_nulls_and_groups() {
    let dir = temp_dir("feat_count_group");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {city: "X"})"#).unwrap();
    db.execute(r#"CREATE (b:Person {city: "X"})"#).unwrap();
    db.execute(r#"CREATE (c:Person {city: "Y"})"#).unwrap();
    db.execute(r#"CREATE (d:Person)"#).unwrap();

    let r = db
        .execute(r#"MATCH (n:Person) RETURN n.city, COUNT(n.city) ORDER BY n.city"#)
        .unwrap();
    // Groups: X -> 2, Y -> 1, null -> 0.
    assert_eq!(r.rows.len(), 3);
    let x_row = r
        .rows
        .iter()
        .find(|row| row[0] == Value::String("X".to_string()))
        .unwrap();
    assert_eq!(x_row[1], Value::Integer(2));

    db.close();
    cleanup_dir(&dir);
}

// ---------- EXTENDED AGGREGATION: SUM / AVG / MIN / MAX ----------

#[test]
fn sum_integers_skips_nulls() {
    let dir = temp_dir("feat_sum_int");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {age: 30})"#).unwrap();
    db.execute(r#"CREATE (b:Person {age: 20})"#).unwrap();
    db.execute(r#"CREATE (c:Person)"#).unwrap();

    let r = db.execute(r#"MATCH (n:Person) RETURN SUM(n.age)"#).unwrap();
    assert_eq!(r.columns, vec!["sum"]);
    assert_eq!(r.rows, vec![vec![Value::Integer(50)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn sum_widens_to_float_on_mixed_numbers() {
    let dir = temp_dir("feat_sum_float");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {age: 10})"#).unwrap();
    db.execute(r#"CREATE (b:Person {age: 2.5})"#).unwrap();

    let r = db.execute(r#"MATCH (n:Person) RETURN SUM(n.age)"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Float(12.5)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn sum_and_avg_empty_groups_yield_null() {
    let dir = temp_dir("feat_sum_empty");
    let mut db = HiveDb::open(&dir).unwrap();

    let r = db
        .execute(r#"MATCH (n:Missing) RETURN SUM(n.age)"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Null]]);
    let r = db
        .execute(r#"MATCH (n:Missing) RETURN AVG(n.age)"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Null]]);
    let r = db
        .execute(r#"MATCH (n:Missing) RETURN MIN(n.age), MAX(n.age)"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Null, Value::Null]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn avg_always_returns_float() {
    let dir = temp_dir("feat_avg");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {age: 30})"#).unwrap();
    db.execute(r#"CREATE (b:Person {age: 20})"#).unwrap();
    db.execute(r#"CREATE (c:Person)"#).unwrap();

    let r = db.execute(r#"MATCH (n:Person) RETURN AVG(n.age)"#).unwrap();
    assert_eq!(r.columns, vec!["avg"]);
    assert_eq!(r.rows, vec![vec![Value::Float(25.0)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn min_max_numbers_and_strings() {
    let dir = temp_dir("feat_minmax");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {age: 30, name: "C"})"#)
        .unwrap();
    db.execute(r#"CREATE (b:Person {age: 20, name: "A"})"#)
        .unwrap();
    db.execute(r#"CREATE (c:Person {age: 25.5, name: "B"})"#)
        .unwrap();

    let r = db
        .execute(r#"MATCH (n:Person) RETURN MIN(n.age), MAX(n.age)"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(20), Value::Integer(30)]]);
    let r = db
        .execute(r#"MATCH (n:Person) RETURN MIN(n.name), MAX(n.name)"#)
        .unwrap();
    assert_eq!(
        r.rows,
        vec![vec![
            Value::String("A".to_string()),
            Value::String("C".to_string())
        ]]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn aggregates_reject_bad_types() {
    let dir = temp_dir("feat_agg_errors");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "Alice", age: 30})"#)
        .unwrap();
    db.execute(r#"CREATE (b:Person {name: "Bob", age: "old"})"#)
        .unwrap();

    let err = db
        .execute(r#"MATCH (n:Person) RETURN SUM(n.age)"#)
        .unwrap_err();
    assert!(err.to_string().contains("SUM"), "{err}");
    let err = db
        .execute(r#"MATCH (n:Person) RETURN AVG(n.name)"#)
        .unwrap_err();
    assert!(err.to_string().contains("AVG"), "{err}");
    // Mixed numbers and strings in one MIN group.
    let err = db
        .execute(r#"MATCH (n:Person) RETURN MIN(n.age)"#)
        .unwrap_err();
    assert!(err.to_string().contains("MIN"), "{err}");

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn grouped_sum_avg_min_max() {
    let dir = temp_dir("feat_agg_grouped");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {city: "X", age: 10})"#)
        .unwrap();
    db.execute(r#"CREATE (b:Person {city: "X", age: 20})"#)
        .unwrap();
    db.execute(r#"CREATE (c:Person {city: "Y", age: 30})"#)
        .unwrap();

    let r = db
        .execute(
            r#"MATCH (n:Person) RETURN n.city, SUM(n.age), AVG(n.age), MIN(n.age), MAX(n.age) ORDER BY n.city"#,
        )
        .unwrap();
    assert_eq!(r.rows.len(), 2);
    let x = &r.rows[0];
    assert_eq!(x[0], Value::String("X".to_string()));
    assert_eq!(x[1], Value::Integer(30));
    assert_eq!(x[2], Value::Float(15.0));
    assert_eq!(x[3], Value::Integer(10));
    assert_eq!(x[4], Value::Integer(20));
    let y = &r.rows[1];
    assert_eq!(y[0], Value::String("Y".to_string()));
    assert_eq!(y[1], Value::Integer(30));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn with_supports_aggregates() {
    let dir = temp_dir("feat_with_agg");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {age: 10})"#).unwrap();
    db.execute(r#"CREATE (b:Person {age: 20})"#).unwrap();

    let r = db
        .execute(r#"MATCH (n:Person) WITH COUNT(*) AS c RETURN c"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(2)]]);
    let r = db
        .execute(r#"MATCH (n:Person) WITH SUM(n.age) AS s RETURN s"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(30)]]);
    // Empty input still yields one WITH row for ungrouped aggregates.
    let r = db
        .execute(r#"MATCH (n:Missing) WITH COUNT(*) AS c RETURN c"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(0)]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn aggregates_rejected_outside_projections() {
    let dir = temp_dir("feat_agg_where");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {age: 10})"#).unwrap();
    let err = db
        .execute(r#"MATCH (n:Person) WHERE SUM(n.age) > 5 RETURN n"#)
        .unwrap_err();
    assert!(err.to_string().contains("SUM"), "{err}");

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn order_by_aggregate() {
    let dir = temp_dir("feat_agg_order");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {city: "X", age: 10})"#)
        .unwrap();
    db.execute(r#"CREATE (b:Person {city: "X", age: 20})"#)
        .unwrap();
    db.execute(r#"CREATE (c:Person {city: "Y", age: 100})"#)
        .unwrap();

    let r = db
        .execute(r#"MATCH (n:Person) RETURN n.city, SUM(n.age) AS s ORDER BY SUM(n.age) DESC"#)
        .unwrap();
    assert_eq!(r.rows[0][0], Value::String("Y".to_string()));
    assert_eq!(r.rows[1][0], Value::String("X".to_string()));

    db.close();
    cleanup_dir(&dir);
}

// ---------- STEP 19: OPTIONAL MATCH ----------

#[test]
fn optional_match_preserves_rows() {
    let dir = temp_dir("feat_optional");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "Lonely"})"#).unwrap();
    db.execute(r#"CREATE (b:Person {name: "Src"})-[:KNOWS]->(c:Person {name: "Dst"})"#)
        .unwrap();

    let r = db
        .execute(
            r#"MATCH (n:Person) OPTIONAL MATCH (n)-[:KNOWS]->(m) RETURN n.name, m.name ORDER BY n.name"#,
        )
        .unwrap();
    assert_eq!(r.rows.len(), 3);
    // Lonely has no match: m.name is NULL.
    let lonely = r
        .rows
        .iter()
        .find(|row| row[0] == Value::String("Lonely".to_string()))
        .unwrap();
    assert_eq!(lonely[1], Value::Null);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn optional_match_single_node_pattern() {
    let dir = temp_dir("feat_optional_single");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})"#).unwrap();
    let r = db
        .execute(r#"MATCH (n:Person) OPTIONAL MATCH (m:Missing) RETURN n.name, m"#)
        .unwrap();
    assert_eq!(r.rows.len(), 1);
    assert_eq!(r.rows[0][1], Value::Null);

    db.close();
    cleanup_dir(&dir);
}

// ---------- STEP 20: REMOVE AND MULTIPLE LABELS ----------

#[test]
fn multi_label_create_and_match() {
    let dir = temp_dir("feat_multilabel");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person:Employee {name: "A"})"#)
        .unwrap();
    let r = db
        .execute(r#"MATCH (n:Person:Employee) RETURN n.name"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("A".to_string())]]);
    // Single-label match on the secondary label also finds the node.
    let r = db.execute(r#"MATCH (n:Employee) RETURN n.name"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::String("A".to_string())]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn remove_label_and_property() {
    let dir = temp_dir("feat_remove");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person:Employee {name: "A", age: 30})"#)
        .unwrap();
    db.execute(r#"MATCH (n:Person) REMOVE n:Employee"#).unwrap();
    let r = db.execute(r#"MATCH (n:Employee) RETURN n"#).unwrap();
    assert_eq!(r.rows.len(), 0);
    let r = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(r.rows.len(), 1);

    db.execute(r#"MATCH (n:Person) REMOVE n.age"#).unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.age"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Null]]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn remove_is_transactional() {
    let dir = temp_dir("feat_remove_tx");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "A"})"#).unwrap();
    // Failed REMOVE (unknown variable for edge label) rolls back.
    let before = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(before.rows.len(), 1);
    let err = db
        .execute(r#"MATCH (n:Person) REMOVE x:Person"#)
        .unwrap_err();
    assert!(err.to_string().contains("unknown variable"), "{err}");
    let after = db.execute(r#"MATCH (n:Person) RETURN n.name"#).unwrap();
    assert_eq!(after.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

// ---------- STEP 21: VARIABLE-LENGTH TRAVERSAL ----------

#[test]
fn var_length_bounded_traversal() {
    let dir = temp_dir("feat_varlen");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})"#).unwrap();
    db.execute(r#"CREATE (b:Person {name: "B"})"#).unwrap();
    db.execute(r#"CREATE (c:Person {name: "C"})"#).unwrap();
    db.execute(r#"CREATE (d:Person {name: "D"})"#).unwrap();
    db.execute(
        r#"MATCH (a:Person {name: "A"}) MATCH (b:Person {name: "B"}) CREATE (a)-[:KNOWS]->(b)"#,
    )
    .unwrap();
    db.execute(
        r#"MATCH (a:Person {name: "B"}) MATCH (b:Person {name: "C"}) CREATE (a)-[:KNOWS]->(b)"#,
    )
    .unwrap();
    db.execute(
        r#"MATCH (a:Person {name: "C"}) MATCH (b:Person {name: "D"}) CREATE (a)-[:KNOWS]->(b)"#,
    )
    .unwrap();

    // A -1-> B -1-> C -1-> D: *1..2 from A reaches B and C but not D.
    let r = db
        .execute(r#"MATCH (a:Person {name: "A"})-[:KNOWS*1..2]->(b) RETURN b.name ORDER BY b.name"#)
        .unwrap();
    let names: Vec<String> = r
        .rows
        .iter()
        .map(|row| match &row[0] {
            Value::String(s) => s.clone(),
            other => panic!("expected string, got {other:?}"),
        })
        .collect();
    assert_eq!(names, vec!["B".to_string(), "C".to_string()]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn var_length_handles_cycles() {
    let dir = temp_dir("feat_varlen_cycle");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})"#).unwrap();
    db.execute(r#"CREATE (b:Person {name: "B"})"#).unwrap();
    db.execute(
        r#"MATCH (a:Person {name: "A"}) MATCH (b:Person {name: "B"}) CREATE (a)-[:KNOWS]->(b)"#,
    )
    .unwrap();
    db.execute(
        r#"MATCH (a:Person {name: "B"}) MATCH (b:Person {name: "A"}) CREATE (a)-[:KNOWS]->(b)"#,
    )
    .unwrap();

    // Cycle A<->B with unbounded traversal must terminate (guardrail MAX 8).
    let r = db
        .execute(r#"MATCH (a:Person {name: "A"})-[:KNOWS*]->(b) RETURN b.name"#)
        .unwrap();
    assert!(!r.rows.is_empty());
    assert!(r.rows.len() <= 8, "guardrail exceeded: {}", r.rows.len());

    db.close();
    cleanup_dir(&dir);
}

// ---------- NULL AND NEGATIVE LITERALS ----------

#[test]
fn null_literal_and_negative_numbers() {
    let dir = temp_dir("feat_null_neg");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (n:Person {name: "A", score: -5})"#)
        .unwrap();
    let r = db.execute(r#"MATCH (n:Person) RETURN n.score"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(-5)]]);
    let r = db
        .execute(r#"MATCH (n:Person) WHERE n.missing = NULL RETURN n.name"#)
        .unwrap();
    // NULL = NULL is true in this engine's equality semantics.
    assert_eq!(r.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

// ---------- STEP 23: OBSERVABILITY ----------

#[test]
fn explain_stats_and_integrity() {
    let dir = temp_dir("feat_observe");
    let mut db = HiveDb::open(&dir).unwrap();

    db.execute(r#"CREATE (a:Person {name: "A"})-[:KNOWS]->(b:Person {name: "B"})"#)
        .unwrap();
    let plan = db
        .explain(r#"MATCH (a:Person)-[:KNOWS]->(b) RETURN a, b"#)
        .unwrap();
    assert!(plan.contains("ScanNodes"), "{plan}");
    assert!(plan.contains("TraverseEdges"), "{plan}");

    let stats = db.stats().unwrap();
    assert_eq!(stats.live_nodes, 2);
    assert_eq!(stats.live_edges, 1);

    let problems = db.check_integrity().unwrap();
    assert!(problems.is_empty(), "{problems:?}");
    let index_problems = db.check_index_consistency().unwrap();
    assert!(index_problems.is_empty(), "{index_problems:?}");

    db.close();
    cleanup_dir(&dir);
}
