use hive::prelude::{Executor, HiveDb, Property, Value, parse, plan};
use hive::value;
use std::fs;
use std::io;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = example_db_path("hive-social-graph");
    let _ = fs::remove_dir_all(&db_path);

    let mut db = HiveDb::open(&db_path)?;

    let alice = create_person(&mut db, "Alice", 30)?;
    let bob = create_person(&mut db, "Bob", 32)?;
    let cara = create_person(&mut db, "Cara", 28)?;

    db.create_edge(
        alice,
        bob,
        "KNOWS",
        vec![property("since", Value::Integer(2020))],
    )?;
    db.create_edge(
        bob,
        cara,
        "KNOWS",
        vec![property("since", Value::Integer(2022))],
    )?;
    db.create_edge(alice, cara, "FOLLOWS", vec![])?;

    run_query(
        &mut db,
        "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a.name AS person, b.name AS knows",
    )?;

    run_query(
        &mut db,
        "MATCH (a:Person)-[:KNOWS*1..2]->(b:Person) WHERE a.name = \"Alice\" RETURN b.name AS reachable",
    )?;

    let info = db.info()?;
    println!(
        "stats: {} live nodes, {} live edges",
        info.live_node_count, info.live_edge_count
    );

    db.close();
    Ok(())
}

fn create_person(db: &mut HiveDb, name: &str, age: i64) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(db.create_node(
        "Person",
        vec![
            property("name", Value::String(name.to_string())),
            property("age", Value::Integer(age)),
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
