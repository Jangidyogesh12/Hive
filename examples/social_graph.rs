use hive::prelude::HiveDb;
use hive::query::{parser::parse, planner::plan};
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = example_db_path("hive-social-graph");
    let _ = fs::remove_dir_all(&db_path);

    let mut db = HiveDb::open(&db_path)?;
    db.begin()?.commit()?;

    plan_query("CREATE (alice:Person {name: \"Alice\", age: 30})")?;
    plan_query("CREATE (bob:Person {name: \"Bob\", age: 32})")?;
    plan_query("CREATE (alice)-[:KNOWS {since: 2020}]->(bob)")?;
    plan_query("MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a.name AS person, b.name AS knows")?;

    db.close();
    Ok(())
}

fn plan_query(query: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n{query}");
    let statement = parse(query)?;
    let query_plan = plan(statement)?;
    println!("{query_plan:#?}");
    Ok(())
}

fn example_db_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}
