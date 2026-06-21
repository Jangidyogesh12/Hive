# Hive Architecture

Hive is a local-first graph database implemented as a Rust workspace. The core design goal is to keep the database embeddable and durable without requiring a server process.

## Workspace Layout

```text
hive/
├── bindings/
│   └── rust/          # Public Rust crate (`hive`)
├── cli/               # Interactive shell and one-shot query runner
├── core/              # Storage engine, query engine, WAL, transactions
├── docs/              # Contributor-facing documentation
├── examples/          # End-to-end examples and demos
├── perf/              # Benchmarks and performance notes
├── public/            # Brand assets used by docs
├── scripts/           # Local automation
├── testing/           # Integration/system test crates
└── tools/             # Repository tools
```

## Crate Boundaries

- `core/`: source of truth for storage, indexes, query execution, WAL, recovery, and transactions
- `bindings/rust/`: stable public Rust entrypoint that re-exports `hive_core` as crate `hive`
- `cli/`: executable crate that depends on `hive_core` and runs Cypher through parser, planner, and executor
- `testing/core/`: integration tests for the engine APIs and query behavior

This shape keeps the database engine independent from user interfaces and future language bindings.

## Query Pipeline

```text
Cypher string
    |
    v
pest grammar + parser
    |
    v
AST (`core/query/ast.rs`)
    |
    v
Query planner (`core/query/planner.rs`)
    |
    v
QueryPlan steps
    |
    v
Executor (`core/query/executor.rs`)
    |
    v
HiveDb API calls (`core/db/hive_db.rs`)
    |
    v
Storage, indexes, and WAL
```

The parser validates syntax and builds an AST. The planner translates AST statements into `QueryPlan` steps such as node scans, edge traversals, filters, projections, updates, and deletes. The executor evaluates those steps against `HiveDb` and returns a `QueryResult` that can be rendered as an ASCII table.

## Storage Model

Hive stores a property graph in flat binary files inside a database directory.

- Nodes and edges are fixed-width records addressed by numeric IDs.
- Properties are fixed-width records linked into per-node or per-edge chains.
- Strings are stored separately as length-prefixed data and referenced by offset.
- Labels and edge types share the label store so records can keep compact numeric IDs.
- Deleted nodes and edges are marked with a flag and their IDs are added to free lists for reuse.

The storage format is documented in `docs/storage.md`.

## Indexing

`IndexStore` maintains persisted/rebuildable indexes for common query paths:

- Label index: `label_id -> Vec<NodeId>`
- Node property equality index: `(key_hash, normalized_value) -> Vec<NodeId>`
- Edge type index: `edge_type_id -> Vec<EdgeId>`

The executor uses these indexes for node candidate selection before falling back to full scans for non-indexable predicates.

## Durability

Hive writes logical mutation intents to the WAL before mutating store files. After durable writes, it records checkpoints and can truncate clean WAL state on reopen.

On open, Hive reads entries after the last checkpoint, replays them through no-WAL recovery helpers, rebuilds derived state such as indexes and free lists where needed, checkpoints, and then truncates the WAL.

Transactions buffer mutations in memory and commit them as one grouped WAL entry. Rollback discards the buffered mutations.

## CLI Flow

The CLI opens a database directory and either runs one query with `--execute` or starts a REPL. REPL commands such as `.open`, `.status`, `.help`, and `.exit` are handled by the CLI; all other input is treated as Cypher and passed to the query pipeline.

## Current Limitations

- The public Rust crate currently re-exports `hive_core` directly; a narrower ergonomic API is still planned.
- Relationship property indexes are not implemented yet.
- Planner-level index selection is still basic; most index usage happens during executor node scans.
- The CLI currently uses standard input rather than `rustyline` history and arrow-key support.
