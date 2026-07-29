<p align="center">
  <img src="public/hive_logo.svg" alt="Hive" />
</p>

# Hive

Hive is a local-first, serverless, Cypher-compatible graph database built in Rust.

Hive stores a property graph directly on local disk using flat binary files. It is designed for embedded applications, local tools, demos, and experiments that need graph-shaped data without operating a database server.

## Status

> **Note:** Hive is under active development and is not ready for production use.
> The storage format, API, and query language are still evolving and may change
> without notice between versions.

Hive is pre-release software targeting `v0.1.0`.

Implemented today:

- Persistent node, edge, property, string, label, metadata, free-list, index, and WAL files
- Programmatic Rust API through the `hive` crate
- Hand-written recursive descent Cypher parser, planner, executor, and ASCII result rendering
- Node label/property indexes and edge type indexes
- Write-ahead log, checkpointing, crash recovery, and basic transactions
- CLI REPL and one-shot query execution
- Transactional metadata (labels, property keys)
- Property key dictionary with collision-safe identity
- Whole-entity return (`RETURN n` produces a map with id, label, properties)
- Adjacency chains for efficient edge traversal
- Persistent freelist and record reuse
- CI quality gates for formatting, clippy, and tests
- Example programs (social graph, knowledge graph)

Still pending before `v0.1.0`:

- B-tree page storage for durable indexes
- Index types and maintenance
- Unique constraints
- Production-safe MERGE
- Query parameters
- WITH pipeline clause, aggregation, OPTIONAL MATCH
- Variable-length traversal
- Concurrency and isolation
- Release packaging and crates.io publishing

## Workspace Layout

This repository follows a Rust workspace layout:

```text
.
├── Cargo.toml         # Workspace manifest
├── bindings/
│   └── rust/          # Public Rust API crate (`hive`)
├── cli/               # CLI crate (`hive` binary)
├── core/              # Database engine crate (`hive_core`)
├── docs/              # Architecture, query, and storage docs
├── examples/          # Example apps and usage samples
├── parser/            # Hand-written Cypher parser crate (`hive_parser`)
├── perf/              # Performance harnesses
├── public/            # Project assets
├── scripts/           # Developer automation
├── testing/           # Integration/system test harnesses
└── tools/             # Repository tools
```

## Packages

- `hive_core`: core database engine, storage layer, query engine, WAL, and transactions
- `hive_parser`: hand-written recursive descent parser for the Cypher-like query language
- `hive`: public Rust API crate that re-exports `hive_core` and `hive_parser`
- `hive_cli`: command-line entrypoint for running Cypher queries against a local Hive database
- `hive_core_testing`: integration test crate under `testing/rust`

## Quick Start

Run the full workspace test suite:

```bash
cargo test --workspace
```

Start the CLI REPL:

```bash
cargo run -p hive_cli -- --db ./.hive
```

Run a one-off query:

```bash
cargo run -p hive_cli -- --db ./.hive --execute "CREATE (n:Person {name: \"Alice\", age: 30})"
cargo run -p hive_cli -- --db ./.hive --execute "MATCH (n:Person) RETURN n.name AS name, n.age AS age"
```

Inside the REPL:

```text
hive> .help
hive> .status
hive> CREATE (n:Person {name: "Alice", age: 30})
hive> MATCH (n:Person) RETURN n
hive> .exit
```

Example REPL session:

```text
$ cargo run -p hive_cli -- --db ./.hive
Connected to ./.hive
Use .help for commands.
hive> CREATE (n:Person {name: "Alice", age: 30})
hive> MATCH (n:Person) RETURN n.name AS name, n.age AS age
+-------+-----+
| name  | age |
+-------+-----+
| Alice | 30  |
+-------+-----+
hive> .status
Connected to ./.hive
hive> .exit
```

## Rust API

The public crate is `hive`. The most common application imports are available through `hive::prelude`.

```rust
use hive::prelude::{HiveDb, Property, Value};
use hive::value;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::Path::new("./.hive-api-example");
    let mut db = HiveDb::open(path)?;

    let value = Value::String("Alice".to_string());
    let (value_type, value_inline) = value.to_inline_bytes();

    let alice = db.create_node(
        "Person",
        vec![Property {
            key_value: "name".to_string(),
            key_hash: value::hash_key("name"),
            value_type,
            value_inline,
        }],
    )?;

    db.set_node_property(alice, "age", Value::Integer(30))?;

    assert_eq!(db.get_node_property(alice, "age")?, Some(Value::Integer(30)));
    let info = db.info()?;
    assert_eq!(info.live_node_count, 1);
    db.close();

    Ok(())
}
```

For Cypher from Rust, use the parser, planner, and executor directly:

```rust
use hive::HiveDb;
use hive::query::{parser::parse, planner::plan};
use hive::query::executor::Executor;

fn run_query(db: &mut HiveDb, query: &str) -> Result<(), String> {
    let statement = parse(query).map_err(|e| e.to_string())?;
    let query_plan = plan(statement).map_err(|e| e.to_string())?;
    let result = Executor::new(db)
        .execute(query_plan)
        .map_err(|e| e.to_string())?;

    if !result.columns.is_empty() {
        println!("{result}");
    }

    Ok(())
}
```

## Supported Cypher Subset

Hive supports a practical subset of Cypher:

- `CREATE (n:Label {key: value})`
- `CREATE (a:Label)-[:TYPE]->(b:Label)`
- `MERGE (n:Label {key: value})`
- `MATCH (n:Label) WHERE n.key = value RETURN n.key AS alias`
- Directed, incoming, undirected, chained, and variable-length relationship matches
- `SET n.key = value`
- `DELETE n` / `DETACH DELETE n`
- `ORDER BY n.key ASC/DESC`
- `SKIP n` / `LIMIT n`
- Literals: `NULL`, integers, floats, booleans, and double-quoted strings
- Operators: `=`, `<>`, `>`, `>=`, `<`, `<=`, `AND`, `OR`, `NOT`
- Whole-entity return: `RETURN n` produces a map with id, label, and properties

See `docs/cypher.md` for examples and known limitations.

## Storage Format

Hive stores each database as a directory of binary files:

- `meta.hive`: header, version, and record counters
- `nodes.hive`: fixed-width node records
- `edges.hive`: fixed-width edge records and adjacency links
- `props.hive`: fixed-width property records chained from nodes and edges
- `strings.hive`: length-prefixed strings for labels, property keys, and long values
- `labels.hive`: label/type ID mappings
- `indexes.hive`: persisted label, node property, and edge type indexes
- `wal.hive`: write-ahead log for recovery
- free-list pages for reusable node and edge IDs

See `docs/storage.md` for more detail.

## Development

Useful commands:

```bash
cargo test --workspace
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo doc --workspace --no-deps
```

Test structure:

- Core engine tests live in `testing/rust/core/` and run via `cargo test -p hive_core_testing`
- Parser tests: `testing/rust/core/parser/` (79 tests covering all supported syntax)
- Planner tests: `testing/rust/core/query/planner_test.rs` (42 tests)
- Executor tests: `testing/rust/core/query/executor_test.rs` (69 tests)
- Storage tests: `testing/rust/core/db/` (node, edge, property, WAL, freelist, label tests)
- For normal development, prefer `cargo test --workspace`

CI runs formatting, clippy, and tests on every push and pull request (`.github/workflows/ci.yml`).

More implementation detail is in `docs/architecture.md`, `docs/cypher.md`, `docs/storage.md`, and `plan.md`.
