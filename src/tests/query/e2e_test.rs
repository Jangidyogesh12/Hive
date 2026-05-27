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

    let stmt = parse("MATCH (n:Person {name: \"Alice\"})-[:KNOWS*1..3]->(m:Person) RETURN m.name").unwrap();
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

    assert_eq!(names, vec!["Raj".to_string(), "Ram".to_string(), "Riya".to_string()]);

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

fn parse_and_exec(input: &str, db: &mut HiveDb) {
    let stmt = parse(input).unwrap();
    let query_plan = plan(stmt).unwrap();
    let mut executor = Executor::new(db);
    executor.execute(query_plan).unwrap();
}

fn match_first_node(name: &str, db: &mut HiveDb) -> u64 {
    let query = format!(
        "MATCH (n:Person) WHERE n.name = \"{}\" RETURN n",
        name
    );
    let stmt = parse(&query).unwrap();
    let query_plan = plan(stmt).unwrap();
    let mut executor = Executor::new(db);
    let result = executor.execute(query_plan).unwrap();
    if let crate::value::Value::Integer(id) = result.rows[0][0] {
        id as u64
    } else {
        panic!("Expected integer node ID");
    }
}
