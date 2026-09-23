<p align="center">
  <img src="public/hive_logo.svg" alt="Hive" />
</p>

# Hive

Hive is a local-first, serverless, Cypher-compatible graph database built in Rust.

Hive stores a property graph directly on local disk using a paged binary file. It is designed for embedded applications, local tools, demos, and experiments that need graph-shaped data without operating a database server.

## Status

> **Note:** Hive is under active development and is not ready for production use.
> The storage format, API, and query language are still evolving and may change
> without notice between versions.

Hive is pre-release software targeting `v0.1.0`. 419 tests pass on `cargo test --workspace`.

Implemented today:

- Paged storage engine (`hive.db` + `wal.hive`) with slotted pages, overflow strings, and page cache
- Programmatic Rust API through the `hive` crate (re-exports `hive_core`)
- Hand-written recursive descent Cypher parser (`hive_parser`) with `miette` diagnostics
- Planner, executor, and ASCII table result rendering
- Durable B+tree page storage (leaf/interior split, root growth, range scan)
- Index catalog and secondary indexes: node label, edge type, node property, edge property (B+tree-backed, transactionally maintained)
- Unique constraints on `(label, property_key)` with rollback and recovery safety
- Write-ahead log, checkpointing, crash recovery, and transactions (read-only `commit_readonly` path avoids WAL)
- CLI REPL with `rustyline` history, `.open` / `.status` / `.help` / `.exit`
- Transactional metadata (labels, property keys)
- Property key dictionary with collision-safe `key_id` identity (no hash-only lookup)
- Whole-entity return (`RETURN n` / `RETURN r` produces `Value::Map` with `id`, `label`/`type`, `src`/`dst`, `properties`)
- Adjacency chains (`first_out_edge` / `first_in_edge` + `next_out` / `next_in`) for efficient one-hop traversal
- Persistent freelist and record reuse (survives reopen/recovery, with page compaction)
- Node/edge/property scan and delete semantics with rollback/reopen/recovery coverage
- Parser / planner / executor production test coverage (parser multi-file suite, 42 planner tests, 69 executor tests)
- CI quality gates for formatting, clippy, and tests
- Example programs (`social_graph`, `knowledge_graph`)

Still pending before `v0.1.0`:

- Production-safe `MERGE` (unique-index-backed node MERGE, relationship MERGE, `ON CREATE SET` / `ON MATCH SET`)
- Query parameters (`$name`)
- `WITH` pipeline clause, aggregation (`COUNT`), `OPTIONAL MATCH`
- `REMOVE` and multiple labels per node
- Variable-length traversal (`*`, `*min..max` — parsed but planner currently rejects with `variable-length relationship execution is not implemented`)
- Concurrency and isolation (`Arc<RwLock<HiveDb>>` → page-level locks / MVCC)
- Observability and integrity tools (page/WAL inspector, `EXPLAIN`, index consistency checker)
- Release packaging and crates.io publishing

See `plan.md` for the ordered implementation steps (Steps 1–14 complete, Steps 15–24 pending).

## Workspace Layout

This repository follows a Rust workspace layout:

```text
.
├── Cargo.toml         # Workspace manifest
├── bindings/
│   └── rust/          # Public Rust API crate (`hive`)
├── cli/               # CLI crate (`hive_cli` binary, `hive` bin)
├── core/              # Database engine crate (`hive_core`)
├── docs/              # Architecture, query, and storage docs
├── examples/          # Example apps and usage samples
├── parser/            # Hand-written Cypher parser crate (`hive_parser`)
├── perf/              # Criterion benchmarks
├── public/            # Project assets
├── scripts/           # Developer automation
├── testing/           # Integration/system test harnesses
│   └── rust/          # `hive_core_testing` crate
└── tools/             # Repository tools
```

Database files live inside the directory you pass to `HiveDb::open` (e.g. `./.hive/hive.db` + `./.hive/wal.hive`).

## Packages

- `hive_core`: core database engine, paged storage, B+tree, WAL, transactions, query engine
- `hive_parser`: hand-written recursive descent parser for the Cypher-like query language (`miette` + `thiserror`)
- `hive`: public Rust API crate that re-exports `hive_core` (and `hive_parser` via `hive_core::query::parser`)
- `hive_cli`: command-line REPL (`cargo run -p hive_cli`) built on `hive_core` + `rustyline`
- `hive_core_testing`: integration test crate under `testing/rust` (run via `cargo test --workspace` or `cargo test -p hive_core_testing`)

## Quick Start

Run the full workspace test suite (419 tests):

```bash
cargo test --workspace
```

Start the CLI REPL (default path `./.hive`):

```bash
cargo run -p hive_cli -- --db ./.hive
# positional path also works:
cargo run -p hive_cli -- ./.hive
```

Inside the REPL:

```text
hive> .help
hive> .help commands
hive> .help path
hive> .status
hive> .open ./another-db
hive> CREATE (n:Person {name: "Alice", age: 30})
hive> MATCH (n:Person) RETURN n.name AS name, n.age AS age
hive> MATCH (n:Person) RETURN n
hive> MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a.name, b.name
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

Run the bundled examples (temp directory, no `--db` flag needed):

```bash
cargo run -p hive --example social_graph
cargo run -p hive --example knowledge_graph
```

## Rust API

The public crate is `hive`. The most common imports are available through `hive::prelude`.

Simplest path — Cypher via `HiveDb::execute`:

```rust
use hive::HiveDb;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut db = HiveDb::open(std::path::Path::new("./.hive-api-example"))?;
    db.execute("CREATE (n:Person {name: \"Alice\", age: 30})")?;
    let result = db.execute("MATCH (n:Person) WHERE n.age = 30 RETURN n.name AS name, n.age AS age")?;
    println!("{result}");
    db.close();
    Ok(())
}
```

Lower-level storage API:

```rust
use hive::{HiveDb, Value};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut db = HiveDb::open(std::path::Path::new("./.hive-api-example"))?;
    let label_id = db.register_label("Person")?;
    let alice = db.create_node_with_label(label_id)?;
    db.set_node_property(alice, "name", &Value::String("Alice".into()))?;
    db.set_node_property(alice, "age", &Value::Integer(30))?;
    assert_eq!(db.get_node_property(alice, "age")?, Value::Integer(30));
    db.close();
    Ok(())
}
```

For explicit parse → plan → execute:

```rust
use hive::HiveDb;
use hive::query::{parser::parse, planner::plan, executor::execute};

fn run_query(db: &mut HiveDb, query: &str) -> Result<(), String> {
    let statement = parse(query).map_err(|e| e.to_string())?;
    let query_plan = plan(statement).map_err(|e| e.to_string())?;
    let result = execute(&query_plan, db).map_err(|e| e.to_string())?;
    if !result.columns.is_empty() {
        println!("{result}");
    }
    Ok(())
}
```

Indexes and unique constraints are also available directly:

```rust
use hive::HiveDb;
# fn demo() -> Result<(), Box<dyn std::error::Error>> {
let mut db = HiveDb::open(std::path::Path::new("./.hive"))?;
db.create_node_label_index("Person")?;
db.create_node_property_index(Some("Person"), "name")?;
db.create_unique_constraint("Person", "email")?;
# Ok(()) }
```

## Supported Cypher Subset

Hive supports a practical subset of Cypher (see `docs/cypher.md`):

- `CREATE (n:Label {key: value})`
- `CREATE (a:Label)-[:TYPE {key: value}]->(b:Label)` (single relationship segment)
- `MERGE (n:Label {key: value})` — single-node MERGE; path MERGE not yet supported
- `MATCH (n:Label)` / `MATCH (n:Label {key: value})` / `MATCH (n) WHERE n.key = value RETURN ...`
- Relationship patterns: directed `->`, incoming `<-`, undirected `-`, chained `(a)-[:KNOWS]->(b)-[:WORKS_AT]->(c)`
- Variable-length `*1..3` etc. is parsed but currently rejected by the planner (`variable-length relationship execution is not implemented`)
- `SET n.key = value` (including edge properties)
- `DELETE n` / `DETACH DELETE n`
- `ORDER BY n.key ASC/DESC`, `SKIP n`, `LIMIT n`
- Literals: `NULL`, integers, floats, booleans, double-quoted strings
- Operators: `=`, `<>`, `>`, `>=`, `<`, `<=`, `AND`, `OR`, `NOT`
- Whole-entity return: `RETURN n` / `RETURN r` produces a map with `id`, label/type, and properties; `RETURN n.name AS alias` supported

Known limitations: `MATCH` requires `RETURN` for reads; no `WITH`, `COUNT`/aggregation, `OPTIONAL MATCH`, `REMOVE`, or query parameters yet.

## Storage Format

Each database is a directory containing:

| File | Purpose |
|---|---|
| `hive.db` | Paged store (4 KiB pages): meta page, slotted data pages (nodes/edges/properties), B+tree index pages, overflow string pages, freelist pages |
| `wal.hive` | Write-ahead log (length-delimited entries with checksums, checkpoint/truncate on reopen) |

Page types (`core/storage/page/format.rs`): `Meta` (magic + version 3 + counters + `freelist_head` + `root_index_page`), `DataNode` / `DataEdge` / `DataProperty`, `Overflow`, `Freelist`, `BTreeNode`.

Record layout (`core/storage/page/record.rs`):

- Node: `id`, `label_id`, `first_out_edge`, `first_in_edge`, `first_property`, flags (deleted)
- Edge: `id`, `src`, `dst`, `type_id`, `next_out_edge`, `next_in_edge`, `first_property`, flags
- Property: `key_id` (via transactional `PropertyKeyStore`), value type + 15-byte inline buffer or overflow offset, `next_property`

Strings/labels: length-prefixed in overflow pages; labels and property keys are dictionary-encoded to `label_id` / `key_id`.

Adjacency: linked edge chains (no separate adjacency list).

Indexes: `indexes.hive` concept is now the B+tree + `IndexCatalog` inside `hive.db` (catalog B+tree keyed by `(entity_kind, label_id, property_key_id)` → root page).

See `docs/storage.md` and `docs/architecture.md` for more detail.

## Indexes and Constraints

```rust
db.create_node_label_index("Person")?;
db.create_edge_type_index("KNOWS")?;
db.create_node_property_index(Some("Person"), "age")?; // per-label
db.create_node_property_index(None, "age")?;            // global
db.create_edge_property_index(Some("KNOWS"), "since")?;
db.create_unique_constraint("Person", "email")?;
```

- Indexes are maintained transactionally on create/set/delete and used by `ScanNodes` when a planner hint matches.
- Full-scan fallback is always available for correctness comparison (see `testing/rust/core/index_test.rs`).
- Unique constraints enforce one-to-one `(label, key)` → node mapping and surface `DbError` on conflict; they are rollback- and recovery-safe.
- Relationship/edge indexes are maintained but `TraverseEdges` still uses adjacency chains (planner hint for traversal is future work).

## Development

Useful commands:

```bash
cargo test --workspace
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo check --workspace --all-targets
cargo doc --workspace --no-deps
```

Test structure:

- All engine tests live in `testing/rust/core/` and run via `cargo test -p hive_core_testing` (or `cargo test --workspace`)
- Parser tests: `testing/rust/core/parser/` (9 files, 79 tests)
- Planner tests: `testing/rust/core/query/planner_test.rs` (42 tests)
- Executor tests: `testing/rust/core/query/executor_test.rs` (69 tests)
- Storage tests: `testing/rust/core/db/` (node, edge, property, WAL, freelist, label tests)
- B-tree tests: `testing/rust/core/btree_test.rs` (14 tests)
- Index tests: `testing/rust/core/index_test.rs` (12 tests)
- Constraint tests: `testing/rust/core/constraint_test.rs`
- For normal development, prefer `cargo test --workspace` (currently 419 tests)

CI runs formatting, clippy, and tests on every push and pull request (`.github/workflows/ci.yml`).

More implementation detail is in `docs/architecture.md`, `docs/cypher.md`, `docs/storage.md`, and `plan.md`.
