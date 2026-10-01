# Hive Architecture

Hive is a local-first graph database implemented as a Rust workspace. The core design goal is to keep the database embeddable and durable without requiring a server process.

## Workspace Layout

```text
hive/
├── bindings/
│   └── rust/          # Public Rust crate (`hive`)
├── cli/               # Interactive REPL shell (`hive_cli`)
├── core/              # Storage engine, query engine, WAL, transactions
├── docs/              # Contributor-facing documentation
├── examples/          # End-to-end examples and demos
├── perf/              # Criterion benchmarks
├── public/            # Brand assets used by docs
├── scripts/           # Local automation
├── testing/
│   └── rust/          # Integration test crate (`hive_core_testing`)
└── tools/             # Repository tools
```

A database is a directory containing `hive.db` (paged store) and `wal.hive`
(write-ahead log), opened via `HiveDb::open(path)`.

## Crate Boundaries

- `parser/` (`hive_parser`): hand-written recursive-descent lexer/parser for the Cypher subset, with `miette` diagnostics. Owns the AST (`parser/src/ast.rs`).
- `core/` (`hive_core`): source of truth for storage, indexes, query planning/execution, WAL, recovery, and transactions.
- `bindings/rust/` (`hive`): stable public Rust entrypoint that re-exports `hive_core` (plus `HiveDb`, `SharedDb`, `Value` in the `prelude`).
- `cli/` (`hive_cli`): REPL built on `hive_core` + `rustyline` history.
- `testing/rust/` (`hive_core_testing`): integration tests for engine APIs and query behavior, run via `cargo test --workspace`.

This shape keeps the database engine independent from user interfaces and future language bindings. For a feature-by-feature contributor reference with implementation notes and code excerpts, see `HIVE.md` at the repository root.

## Query Pipeline

```text
Cypher string
    |
    v
Lexer + recursive-descent parser (`hive_parser`)
    |
    v
AST (`parser/src/ast.rs`, re-exported as `hive_core::query::ast`)
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
Transaction API (`core/transaction.rs` over `HiveDb`)
    |
    v
Storage, indexes, and WAL
```

The parser validates syntax and builds an ordered clause pipeline (`CREATE`, `MATCH`, `OPTIONAL MATCH`, `WITH`, `WHERE`, `SET`, `DELETE`, `REMOVE`, `MERGE` + `ON CREATE/MATCH SET`, `RETURN`). The planner translates each clause into `QueryPlan` steps — `CreateNode`, `CreateRelationship`, `MergeNode`, `MergeRelationship`, `ScanNodes`, `TraverseEdges`, `Filter`, `With`, `SetProperty`, `Delete`, `Remove`, `Return` — while tracking variable scope and deriving node index hints from label/property equality. The executor evaluates those steps as a stream of variable bindings (`Binding::{Node, Edge, Value}` keyed by variable name) inside a single transaction and returns a `QueryResult` rendered as an ASCII table. `HiveDb::explain` renders the plan without executing it.

## Storage Model

Hive stores a property graph in slotted 4 KiB pages inside `hive.db`:

- Node and edge records are variable-width, addressed by packed record IDs `(page_id, slot_id)`. Slot indices are stable across page compaction, so record IDs stay valid.
- Properties are inline entries (`key_id` + type tag + 15-byte buffer) inside the owning node/edge record; strings longer than 15 bytes spill to overflow pages.
- Property keys are dictionary-encoded (`PropertyKeyData` pages mapping `key_id -> name`), so lookups use key identity, never hash equality.
- Nodes carry one primary label plus an `extra_labels` list (`(n:A:B)`); labels/edge types are dictionary-encoded (`LabelData` pages mapping `label_id -> name`).
- Adjacency uses linked edge chains: each node points at its first outgoing/incoming edge, and each edge links to the next edge in both chains. One-hop traversal walks chains; variable-length traversal BFS-expands them with cycle avoidance and an 8-hop guardrail.
- Deleted records are marked dead and their space is reclaimed via a persistent freelist plus slot-stable page compaction.

The storage format is documented in `docs/storage.md`.

## Indexing

Indexes are durable B+tree structures living in `hive.db`, coordinated by an index catalog (itself a B+tree keyed by `(entity_kind, label_id, property_key_id)`):

- Node label index, edge type index, node property index (per-label and global), edge property index (per-type and global).
- Unique constraints on `(label, property_key)` backed by a per-label property index, enforced on create/set/merge with rollback and recovery safety.
- All index writes happen inside the same transaction as the data writes, capturing page before-images, so maintenance is atomic with the mutation.
- The executor uses planner index hints for `ScanNodes` and falls back to full scans otherwise; indexed and full-scan plans return identical results.

## Transactions and Durability

- `HiveDb::begin()` opens a transaction that records page before-images for every touched page (data, dictionary, B-tree, meta). `commit()` writes the dirty pages to the WAL, syncs, and stamps LSNs; `rollback()` restores before-images. Newly allocated pages are freed on rollback.
- Mutating queries run in exactly one transaction and commit once; any execution error rolls everything back, including label/property-key registrations and index updates.
- Read-only plans take a WAL-free `commit_readonly()` path that marks dirty pages clean without appending to the WAL.
- On open, Hive replays committed WAL entries after the last checkpoint (page-image redo), then checkpoints and truncates. `node_count`/`edge_count` in the meta page are allocation counters, never decremented on delete.
- `SharedDb` (`core/db/shared.rs`) pairs the primary handle with an admission `RwLock`: writers take it exclusively, while each read-only query opens a private read-only snapshot (`HiveDb::open_snapshot`) under a shared guard. Snapshots replay committed WAL entries into their own cache and never write the shared files, so readers run concurrently under snapshot isolation while writers stay exclusive.

## CLI Flow

The CLI opens a database directory (`--db <path>` or a positional path, default `./.hive`) and starts a `rustyline` REPL. Dot-commands (`.help`, `.open <path>`, `.status`, `.explain <query>`, `.stats`, `.check`, `.inspect <page>`, `.wal [limit]`, `.quit`/`.exit`) are handled by the CLI; all other input is treated as Cypher and passed through parse → plan → execute, printing the ASCII-table result plus per-stage timing (`QueryMetrics`), or the error.

## Observability

Beyond `explain`/`stats`/checkers, `HiveDb::inspect_page` dumps a byte-level page report (decoded node/edge records, dictionary entries, B-tree cells, freelist entries, meta fields), `HiveDb::inspect_wal` lists WAL entries oldest-first, and `HiveDb::execute_timed` returns per-stage wall time plus row counts (`QueryMetrics`).

## Current Limitations

- The public Rust crate re-exports `hive_core` directly; a narrower ergonomic API is still planned.
- Planner-level index selection is still basic; traversal uses adjacency chains rather than edge indexes.
- Snapshot reads open a fresh handle per query (WAL replay + cold cache each time); a pooled snapshot cache is future work.
- Release packaging and crates.io publishing are deferred (handled separately).
