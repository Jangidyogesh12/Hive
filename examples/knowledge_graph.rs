use hive::prelude::{Executor, HiveDb, Property, Value, parse, plan};
use hive::value;
use std::fs;
use std::io;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = example_db_path("hive-knowledge-graph");
    let _ = fs::remove_dir_all(&db_path);

    let mut db = HiveDb::open(&db_path)?;

    let rust = create_concept(&mut db, "Rust", "language")?;
    let ownership = create_concept(&mut db, "Ownership", "concept")?;
    let borrow_checker = create_concept(&mut db, "Borrow Checker", "concept")?;
    let graph_db = create_concept(&mut db, "Graph Database", "database")?;

    db.create_edge(
        rust,
        ownership,
        "HAS_CONCEPT",
        vec![property("weight", Value::Integer(10))],
    )?;
    db.create_edge(
        ownership,
        borrow_checker,
        "RELATED_TO",
        vec![property("weight", Value::Integer(8))],
    )?;
    db.create_edge(
        graph_db,
        rust,
        "IMPLEMENTED_IN",
        vec![property("project", Value::String("Hive".to_string()))],
    )?;

    run_query(
        &mut db,
        "MATCH (c:Concept) WHERE c.kind = \"concept\" RETURN c.name AS concept",
    )?;

    run_query(
        &mut db,
        "MATCH (a:Concept)-[:RELATED_TO]-(b:Concept) RETURN a.name AS source, b.name AS related",
    )?;

    let implemented_in =
        db.lookup_edge_ids_by_property("project", &Value::String("Hive".to_string()))?;
    println!("\nIMPLEMENTED_IN project=Hive edge ids: {implemented_in:?}");

    db.close();
    Ok(())
}

fn create_concept(
    db: &mut HiveDb,
    name: &str,
    kind: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(db.create_node(
        "Concept",
        vec![
            property("name", Value::String(name.to_string())),
            property("kind", Value::String(kind.to_string())),
        ],
    )?)
}

fn property(key: &str, value: Value) -> Property {
    let (value_type, value_inline) = value.to_inline_bytes();
    Property {
        key_value: key.to_string(),
        key_hash: value::hash_key(key),
        value_type,
        value_inline,
    }
}

fn run_query(db: &mut HiveDb, query: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n{query}");
    let statement = parse(query).map_err(io::Error::other)?;
    let query_plan = plan(statement)?;
    let result = Executor::new(db).execute(query_plan)?;
    if !result.columns.is_empty() {
        println!("{result}");
    }
    Ok(())
}

fn example_db_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}
