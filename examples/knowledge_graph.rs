use hive::prelude::HiveDb;
use hive::query::{parser::parse, planner::plan};
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = example_db_path("hive-knowledge-graph");
    let _ = fs::remove_dir_all(&db_path);

    let mut db = HiveDb::open(&db_path)?;
    db.begin()?.commit()?;

    plan_query("CREATE (rust:Concept {name: \"Rust\", kind: \"language\"})")?;
    plan_query("CREATE (ownership:Concept {name: \"Ownership\", kind: \"concept\"})")?;
    plan_query("CREATE (rust)-[:HAS_CONCEPT {weight: 10}]->(ownership)")?;
    plan_query("MATCH (c:Concept) WHERE c.kind = \"concept\" RETURN c.name AS concept")?;
    plan_query(
        "MATCH (a:Concept)-[:RELATED_TO]-(b:Concept) RETURN a.name AS source, b.name AS related",
    )?;

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
