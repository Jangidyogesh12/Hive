use crate::db::hive_db::HiveDb;
use crate::query::executor::Executor;
use crate::query::parser::parse;
use crate::query::planner::plan;
use crate::tests::utils::utils::{cleanup_dir, temp_dir};
use crate::value::Value;

#[test]
fn end_to_end_create_and_match() {
    let dir = temp_dir("query_e2e");
    let mut db = HiveDb::open(&dir).unwrap();

    let stmt = parse("CREATE (n:Person {name: \"Alice\", age: 30})").unwrap();
    let query_plan = plan(stmt).unwrap();
    let mut executor = Executor::new(&mut db);
    let result = executor.execute(query_plan).unwrap();
    assert!(result.rows.is_empty());

    let stmt = parse("MATCH (n:Person) RETURN n, n.name").unwrap();
    let query_plan = plan(stmt).unwrap();
    let result = executor.execute(query_plan).unwrap();
    assert_eq!(result.columns, vec!["n".to_string(), "n.name".to_string()]);
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][1], Value::String("Alice".to_string()));

    let stmt = parse("MATCH (n:Person) WHERE n.age > 20 RETURN n").unwrap();
    let query_plan = plan(stmt).unwrap();
    let result = executor.execute(query_plan).unwrap();
    assert_eq!(result.rows.len(), 1);

    let stmt = parse("MATCH (n:Person) WHERE n.age < 20 RETURN n").unwrap();
    let query_plan = plan(stmt).unwrap();
    let result = executor.execute(query_plan).unwrap();
    assert_eq!(result.rows.len(), 0);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn end_to_end_create_and_match_with_traversal() {
    let dir = temp_dir("query_traversal");
    let mut db = HiveDb::open(&dir).unwrap();

    parse_and_exec("CREATE (n:Person {name: \"Alice\"})", &mut db);
    parse_and_exec("CREATE (n:Person {name: \"Bob\"})", &mut db);

    let alice_id = match_first_node("Alice", &mut db);
    let bob_id = match_first_node("Bob", &mut db);

    db.create_edge(alice_id, bob_id, "KNOWS", vec![]).unwrap();

    let stmt = parse("MATCH (n:Person)-[:KNOWS]->(m:Person) RETURN n, m").unwrap();
    let query_plan = plan(stmt).unwrap();
    let mut executor = Executor::new(&mut db);
    let result = executor.execute(query_plan).unwrap();
    assert_eq!(result.rows.len(), 1);

    let stmt = parse("MATCH (n:Person)-[e:KNOWS]->(m:Person) RETURN e").unwrap();
    let query_plan = plan(stmt).unwrap();
    let result = executor.execute(query_plan).unwrap();
    assert_eq!(result.columns, vec!["e".to_string()]);
    assert_eq!(result.rows.len(), 1);
    match &result.rows[0][0] {
        Value::String(s) => {
            assert!(s.contains("type:\"KNOWS\""));
            assert!(s.contains("src:"));
            assert!(s.contains("dst:"));
            assert!(s.contains("props:{"));
        }
        other => panic!("Expected edge string, got {:?}", other),
    }

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn end_to_end_variable_hops_traversal() {
    let dir = temp_dir("query_variable_hops");
    let mut db = HiveDb::open(&dir).unwrap();

    parse_and_exec("CREATE (n:Person {name: \"Alice\"})", &mut db);
    parse_and_exec("CREATE (n:Person {name: \"Raj\"})", &mut db);
    parse_and_exec("CREATE (n:Person {name: \"Ram\"})", &mut db);
    parse_and_exec("CREATE (n:Person {name: \"Riya\"})", &mut db);

    let alice = match_first_node("Alice", &mut db);
    let raj = match_first_node("Raj", &mut db);
    let ram = match_first_node("Ram", &mut db);
    let riya = match_first_node("Riya", &mut db);

    db.create_edge(alice, raj, "KNOWS", vec![]).unwrap();
    db.create_edge(raj, ram, "KNOWS", vec![]).unwrap();
    db.create_edge(ram, riya, "KNOWS", vec![]).unwrap();
    db.create_edge(ram, alice, "KNOWS", vec![]).unwrap();

    let stmt = parse("MATCH (n:Person {name: \"Alice\"})-[:KNOWS*1..3]->(m:Person) RETURN m.name")
        .unwrap();
    let query_plan = plan(stmt).unwrap();
    let mut executor = Executor::new(&mut db);
    let result = executor.execute(query_plan).unwrap();

    let mut names: Vec<String> = result
        .rows
        .iter()
        .map(|row| match &row[0] {
            Value::String(s) => s.clone(),
            other => panic!("Expected string name, got {:?}", other),
        })
        .collect();
    names.sort();

    assert_eq!(
        names,
        vec!["Raj".to_string(), "Ram".to_string(), "Riya".to_string()]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn end_to_end_set_and_delete_require_bound_variables() {
    let dir = temp_dir("query_set_delete_err");
    let mut db = HiveDb::open(&dir).unwrap();

    let set_stmt = parse("SET n.age = 31").unwrap();
    let set_plan = plan(set_stmt).unwrap();
    let mut executor = Executor::new(&mut db);
    let set_err = executor.execute(set_plan).unwrap_err();
    assert!(
        set_err
            .to_string()
            .contains("SET requires a preceding MATCH to bind the variable")
    );

    let delete_stmt = parse("DELETE n").unwrap();
    let delete_plan = plan(delete_stmt).unwrap();
    let delete_err = executor.execute(delete_plan).unwrap_err();
    assert!(
        delete_err
            .to_string()
            .contains("DELETE requires a preceding MATCH to bind the variable")
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn end_to_end_incoming_and_undirected_traversal() {
    let dir = temp_dir("query_incoming_undirected");
    let mut db = HiveDb::open(&dir).unwrap();

    parse_and_exec("CREATE (n:Person {name: \"Alice\"})", &mut db);
    parse_and_exec("CREATE (n:Person {name: \"Bob\"})", &mut db);
    parse_and_exec("CREATE (n:Person {name: \"Cara\"})", &mut db);

    let alice = match_first_node("Alice", &mut db);
    let bob = match_first_node("Bob", &mut db);
    let cara = match_first_node("Cara", &mut db);

    db.create_edge(alice, bob, "KNOWS", vec![]).unwrap();
    db.create_edge(cara, alice, "KNOWS", vec![]).unwrap();

    let incoming_stmt =
        parse("MATCH (n:Person)<-[:KNOWS]-(m:Person) WHERE n.name = \"Alice\" RETURN m.name")
            .unwrap();
    let incoming_plan = plan(incoming_stmt).unwrap();
    let mut executor = Executor::new(&mut db);
    let incoming_result = executor.execute(incoming_plan).unwrap();

    let mut incoming_names: Vec<String> = incoming_result
        .rows
        .iter()
        .map(|row| match &row[0] {
            Value::String(s) => s.clone(),
            other => panic!("Expected string name, got {:?}", other),
        })
        .collect();
    incoming_names.sort();
    assert_eq!(incoming_names, vec!["Cara".to_string()]);

    let undirected_stmt =
        parse("MATCH (n:Person)-[:KNOWS]-(m:Person) WHERE n.name = \"Alice\" RETURN m.name")
            .unwrap();
    let undirected_plan = plan(undirected_stmt).unwrap();
    let undirected_result = executor.execute(undirected_plan).unwrap();

    let mut undirected_names: Vec<String> = undirected_result
        .rows
        .iter()
        .map(|row| match &row[0] {
            Value::String(s) => s.clone(),
            other => panic!("Expected string name, got {:?}", other),
        })
        .collect();
    undirected_names.sort();
    assert_eq!(
        undirected_names,
        vec!["Bob".to_string(), "Cara".to_string()]
    );

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn end_to_end_compound_where_with_comparisons_and_not() {
    let dir = temp_dir("query_compound_where");
    let mut db = HiveDb::open(&dir).unwrap();

    parse_and_exec("CREATE (n:Person {name: \"Alice\", age: 30})", &mut db);
    parse_and_exec("CREATE (n:Person {name: \"Bob\", age: 35})", &mut db);
    parse_and_exec("CREATE (n:Person {name: \"Cara\", age: 20})", &mut db);

    let stmt = parse("MATCH (n:Person) WHERE n.age >= 30 AND n.age <= 35 AND n.name <> \"Cara\" AND NOT n.age < 30 RETURN n.name").unwrap();
    let query_plan = plan(stmt).unwrap();
    let mut executor = Executor::new(&mut db);
    let result = executor.execute(query_plan).unwrap();

    let mut names: Vec<String> = result
        .rows
        .iter()
        .map(|row| match &row[0] {
            Value::String(s) => s.clone(),
            other => panic!("Expected string name, got {:?}", other),
        })
        .collect();
    names.sort();

    assert_eq!(names, vec!["Alice".to_string(), "Bob".to_string()]);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn end_to_end_complex_match_chained_relationships() {
    let dir = temp_dir("query_complex_match_chain");
    let mut db = HiveDb::open(&dir).unwrap();

    parse_and_exec("CREATE (n:Person {name: \"Alice\"})", &mut db);
    parse_and_exec("CREATE (n:Person {name: \"Bob\"})", &mut db);
    parse_and_exec("CREATE (n:Company {name: \"Acme\"})", &mut db);

    let alice = match_first_node("Alice", &mut db);
    let bob = match_first_node("Bob", &mut db);
    let acme = match_first_company("Acme", &mut db);

    db.create_edge(alice, bob, "KNOWS", vec![]).unwrap();
    db.create_edge(bob, acme, "WORKS_AT", vec![]).unwrap();

    let stmt =
        parse("MATCH (a:Person)-[:KNOWS]->(b:Person)-[:WORKS_AT]->(c:Company) RETURN a.name, b.name, c.name")
            .unwrap();
    let query_plan = plan(stmt).unwrap();
    let mut executor = Executor::new(&mut db);
    let result = executor.execute(query_plan).unwrap();

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0], Value::String("Alice".to_string()));
    assert_eq!(result.rows[0][1], Value::String("Bob".to_string()));
    assert_eq!(result.rows[0][2], Value::String("Acme".to_string()));

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn end_to_end_merge_is_idempotent_for_same_pattern() {
    let dir = temp_dir("query_merge_idempotent");
    let mut db = HiveDb::open(&dir).unwrap();

    parse_and_exec("MERGE (n:Person {name: \"Alice\"})", &mut db);
    parse_and_exec("MERGE (n:Person {name: \"Alice\"})", &mut db);

    let stmt = parse("MATCH (n:Person) WHERE n.name = \"Alice\" RETURN n").unwrap();
    let query_plan = plan(stmt).unwrap();
    let mut executor = Executor::new(&mut db);
    let result = executor.execute(query_plan).unwrap();
    assert_eq!(result.rows.len(), 1);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn end_to_end_merge_creates_new_node_for_different_properties() {
    let dir = temp_dir("query_merge_different_properties");
    let mut db = HiveDb::open(&dir).unwrap();

    parse_and_exec("MERGE (n:Person {name: \"Alice\"})", &mut db);
    parse_and_exec("MERGE (n:Person {name: \"Bob\"})", &mut db);

    let stmt = parse("MATCH (n:Person) RETURN n").unwrap();
    let query_plan = plan(stmt).unwrap();
    let mut executor = Executor::new(&mut db);
    let result = executor.execute(query_plan).unwrap();
    assert_eq!(result.rows.len(), 2);

    db.close();
    cleanup_dir(&dir);
}

#[test]
fn merge_path_pattern_is_rejected() {
    let stmt = parse("MERGE (a:Person)-[:KNOWS]->(b:Person)").unwrap();
    let err = plan(stmt).unwrap_err();
    assert!(
        err.to_string()
            .contains("MERGE currently supports only single node patterns")
    );
}

fn parse_and_exec(input: &str, db: &mut HiveDb) {
    let stmt = parse(input).unwrap();
    let query_plan = plan(stmt).unwrap();
    let mut executor = Executor::new(db);
    executor.execute(query_plan).unwrap();
}

fn match_first_node(name: &str, db: &mut HiveDb) -> u64 {
    let count = db.node_count().unwrap();
    for id in 0..count {
        let node = db.get_node(id).unwrap();
        if (node.flags & crate::types::DELETED) != 0 {
            continue;
        }
        if node.label != "Person" {
            continue;
        }
        if let Some(Value::String(node_name)) = db.get_node_property(id, "name").unwrap() {
            if node_name == name {
                return id;
            }
        }
    }
    panic!("Expected matching Person node");
}

fn match_first_company(name: &str, db: &mut HiveDb) -> u64 {
    let count = db.node_count().unwrap();
    for id in 0..count {
        let node = db.get_node(id).unwrap();
        if (node.flags & crate::types::DELETED) != 0 {
            continue;
        }
        if node.label != "Company" {
            continue;
        }
        if let Some(Value::String(node_name)) = db.get_node_property(id, "name").unwrap() {
            if node_name == name {
                return id;
            }
        }
    }
    panic!("Expected matching Company node");
}
