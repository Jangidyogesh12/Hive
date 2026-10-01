# HIVE — Contributor Reference

Hive is a local-first, serverless, Cypher-compatible property-graph database
written in Rust. It stores a property graph directly on local disk using a
paged binary file, with no server process: applications embed it through the
`hive` crate.

> Status: pre-release software targeting `v0.1.0`. The storage format, API,
> and query language may still change between versions. 477 tests pass on
> `cargo test --workspace`.

This document is the contributor reference. It explains **every feature**,
**how each one is implemented**, and **where the code lives**, with short
code excerpts. User-facing usage lives in `README.md`; query syntax in
`docs/cypher.md`; on-disk bytes in `docs/storage.md`; big-picture structure
in `docs/architecture.md`.

---

## Table of Contents

1. [Workspace layout and crates](#1-workspace-layout-and-crates)
2. [Opening a database and running queries](#2-opening-a-database-and-running-queries)
3. [Storage engine](#3-storage-engine)
4. [Values and overflow strings](#4-values-and-overflow-strings)
5. [Dictionaries: labels and property keys](#5-dictionaries-labels-and-property-keys)
6. [Adjacency chains](#6-adjacency-chains)
7. [Transactions, WAL, and recovery](#7-transactions-wal-and-recovery)
8. [B-tree indexes, catalog, and unique constraints](#8-b-tree-indexes-catalog-and-unique-constraints)
9. [Query pipeline: parse → plan → execute](#9-query-pipeline-parse--plan--execute)
10. [Core clauses: CREATE, MATCH, WHERE, SET, DELETE](#10-core-clauses-create-match-where-set-delete)
11. [Production-safe MERGE](#11-production-safe-merge)
12. [Query parameters](#12-query-parameters)
13. [WITH pipeline clause](#13-with-pipeline-clause)
14. [Aggregation with COUNT](#14-aggregation-with-count)
15. [OPTIONAL MATCH](#15-optional-match)
16. [REMOVE and multiple labels](#16-remove-and-multiple-labels)
17. [Variable-length traversal](#17-variable-length-traversal)
18. [RETURN shape, ordering, literals, comparison](#18-return-shape-ordering-literals-comparison)
19. [Concurrency with SharedDb](#19-concurrency-with-shareddb)
20. [Observability: EXPLAIN, stats, checkers](#20-observability-explain-stats-checkers)
21. [Public API surface](#21-public-api-surface)
22. [Testing guide](#22-testing-guide)
23. [Pending roadmap (post-v0.1.0 candidates)](#23-pending-roadmap-post-v010-candidates)
24. [How to add a feature](#24-how-to-add-a-feature)

---

## 1. Workspace layout and crates

```text
.
├── bindings/rust/   # Public Rust API crate (`hive`, re-exports `hive_core`)
├── cli/             # CLI crate (`hive_cli` binary)
├── core/            # Database engine crate (`hive_core`)
├── docs/            # Architecture, query, and storage docs
├── examples/        # Example apps (`social_graph`, `knowledge_graph`)
├── parser/          # Cypher parser crate (`hive_parser`)
├── perf/            # Criterion benchmarks
├── testing/rust/    # Integration tests (`hive_core_testing`)
├── tools/           # Repository tools
├── HIVE.md          # This file (contributor reference)
└── README.md        # User-facing overview
```

| Crate | Role |
|---|---|
| `hive_parser` | Hand-written recursive-descent lexer + parser, `miette` diagnostics. Owns the AST (`parser/src/ast.rs`). |
| `hive_core` | Storage, B-tree, WAL, transactions, planner, executor, `HiveDb`, `SharedDb`. |
| `hive` | Stable public entrypoint; re-exports `hive_core` (`hive::prelude` has `HiveDb`, `SharedDb`, `Value`, `QueryMetrics`, …). |
| `hive_cli` | REPL on `hive_core` + `rustyline`. |
| `hive_core_testing` | All engine/query tests; run with `cargo test --workspace`. |

---

## 2. Opening a database and running queries

A database is a **directory** containing `hive.db` (paged store) plus
`wal.hive` (write-ahead log). Three levels of API exist, simplest first:

```rust
use hive::HiveDb;

// Level 1 — Cypher string in, table out.
let mut db = HiveDb::open(std::path::Path::new("./.hive"))?;
db.execute("CREATE (n:Person {name: \"Alice\", age: 30})")?;
let result = db.execute("MATCH (n:Person) RETURN n.name AS name")?;
println!("{result}");
db.close();
```

```rust
use hive::{HiveDb, Value};

// Level 2 — lower-level storage API (no Cypher).
let mut db = HiveDb::open(std::path::Path::new("./.hive"))?;
let label = db.register_label("Person")?;
let alice = db.create_node_with_label(label)?;
db.set_node_property(alice, "age", &Value::Integer(30))?;
assert_eq!(db.get_node_property(alice, "age")?, Value::Integer(30));
db.close();
```

```rust
use hive::HiveDb;
use hive::query::{parser::parse, planner::plan, executor::execute};

// Level 3 — explicit parse → plan → execute.
let statement = parse(query).map_err(|e| e.to_string())?;
let query_plan = plan(statement).map_err(|e| e.to_string())?;
let result = execute(&query_plan, db).map_err(|e| e.to_string())?;
```

---

## 3. Storage engine

### 3.1 Files and pages

Each database directory holds `hive.db` + `wal.hive`
(`core/storage/pager.rs`, `core/db/hive_db.rs`). Every page is exactly
`PAGE_SIZE` (4096) bytes (`core/storage/page/format.rs:4`):

```rust
pub const PAGE_SIZE: usize = 4096;
pub const REGULAR_HEADER_SIZE: usize = 20;
pub const SLOT_ENTRY_SIZE: usize = 4;
pub const CURRENT_VERSION: u32 = 3;
```

Page 0 is the meta page (100-byte `MetaHeader`: magic, version, page size,
allocation counters, root-page pointers, `freelist_head`,
`root_index_page`, checksum, LSN). Every other page starts with a 20-byte
`PageHeader` (type tag, flags, slot count, free-space offset, freeblock
head, checksum, LSN).

Page types (`core/storage/page/format.rs:20`):

```rust
pub enum PageType {
    Meta = 0x00, DataNode = 0x01, DataEdge = 0x02,
    StringData = 0x04,                 // legacy, unused
    LabelData = 0x05, PropertyKeyData = 0x06,
    IndexInterior = 0x0A, IndexLeaf = 0x0B,
    Freelist = 0x0F, Overflow = 0x10,
}
```

### 3.2 Slotted pages and slot-stable compaction

Data, dictionary, and B-tree pages share one slotted layout
(`core/storage/page/layout.rs`): the slot table grows downward from byte 20
(4 bytes per slot: content offset + length; offset 0 = dead), record content
grows upward from the end of the page. `insert_record` appends a slot,
`insert_record_at` reuses a dead slot, `update_record` rewrites in place
when the new bytes fit and otherwise deletes + re-inserts at the same slot.

Compaction is **slot-stable**: live records are repacked contiguously
**without renumbering slots**, so packed record IDs `(page_id, slot_id)`
stay valid — a hard invariant the query engine and adjacency chains rely on.

### 3.3 Node, edge, and property records

Variable-width records (`core/storage/page/record.rs`), addressed by packed
record ID, never by file position:

```rust
pub struct NodeRecord {
    pub id: u64,             // logical node ID (monotonic allocation counter)
    pub label_id: u32,       // primary label, 0 = unlabeled
    pub flags: u32,
    pub first_out_edge: u64, // packed EdgeId or NIL_ID
    pub first_in_edge: u64,
    pub first_property: u64, // reserved
    pub properties: Vec<PropertyEntry>,
    pub extra_labels: Vec<u32>, // (n:A:B) support
}

pub struct PropertyEntry {
    pub key_id: u32,         // property-key dictionary ID (never a hash)
    pub value_type: u8,
    pub value_inline: [u8; 15],
    pub long_value_offset: u64,
}
```

`EdgeRecord` mirrors this with `src`, `dst`, `next_out_edge`,
`next_in_edge` chain links. Serialization is manual little-endian
(`to_bytes` / `from_bytes`); node records end with an
`[extra_label_count: u16][label_id: u32 …]` section, and readers tolerate
files that predate it.

### 3.4 Space reuse (freelist)

Deleted slots are marked dead; pages track them in a page-local freeblock
chain while whole free pages are tracked in persistent `Freelist` pages
chained from the meta `freelist_head`. The freelist is written on sync and
reloaded on open, so freed pages survive restart, and rolled-back
allocations return to the freelist.

---

## 4. Values and overflow strings

`Value` (`core/value.rs:15`) is the single runtime type for properties and
query results:

```rust
pub enum Value {
    Null,
    Integer(i64),
    Float(f64),
    Boolean(bool),
    String(String),
    Map(HashMap<String, Value>), // whole-entity returns only
    List(Vec<Value>),            // whole-entity returns only
}
```

Storable values encode to a type tag + 15-byte inline buffer
(`to_inline_bytes` / `from_bytes`):

```rust
Value::Integer(n) => { buf[..8].copy_from_slice(&n.to_le_bytes()); (INTEGER, buf) }
Value::String(s) if s.len() <= 15 => { /* inline */ (STRING, buf) }
Value::String(_) => (LONG_STRING, [0u8; 15]), // offset stored separately
```

Strings longer than 15 bytes go to `Overflow` pages
(`core/storage/overflow_store.rs`); the entry keeps the page offset.
`Map`/`List` are query-result-only and never stored.

---

## 5. Dictionaries: labels and property keys

Labels/edge types (`LabelData` pages, `core/storage/label_store.rs`) and
property keys (`PropertyKeyData` pages,
`core/storage/property_key_store.rs`) are dictionary-encoded to `u32` IDs.
Both registrations are **transactional**: they capture page before-images,
so a failed query rolls a newly registered label/key back, while committed
ones survive reopen and WAL recovery. Duplicate registration returns the
existing ID. Property reads resolve names through the dictionary and match
entries by `key_id` — hash collisions are impossible by construction.

---

## 6. Adjacency chains

No separate adjacency list exists. Each node stores `first_out_edge` /
`first_in_edge`; each edge stores `next_out_edge` / `next_in_edge`. Edge
creation prepends to both endpoint chains; deletion unlinks from both —
always inside the enclosing transaction. One-hop `MATCH` walks the chains
(`Transaction::get_edges_from_node`); variable-length traversal BFS-expands
them (see §17).

---

## 7. Transactions, WAL, and recovery

`HiveDb::begin()` opens a `Transaction` (`core/transaction.rs`) that
records a **before-image** (full 4 KiB copy) of every page it touches — data,
dictionary, B-tree, and meta pages — plus a list of newly allocated pages:

```rust
pub fn commit(self) -> Result<(), DbError> {
    self.db.commit_tx(self.tx_id)   // WAL-append dirty pages, sync, stamp LSNs
}
pub fn rollback(self) -> Result<(), DbError> {
    self.db.rollback_pages(&self.before_images)
}
/// Read-only commit: no WAL work at all.
pub fn commit_readonly(self) -> Result<(), DbError> {
    self.db.commit_readonly()
}
```

Rules the engine follows:

- Every mutating query runs in **exactly one transaction, committed once**;
  any execution error rolls back nodes, edges, labels, property keys,
  overflow strings, and index updates together.
- Read-only plans (`ScanNodes`/`TraverseEdges`/`Filter`/`With`/`Return`
  only) take the WAL-free `commit_readonly()` path.
- `commit()` appends length-delimited, checksummed page-image entries to
  `wal.hive` (`core/wal/`), syncs, and stamps LSNs; roughly every 64 commits
  it auto-checkpoints.
- On open, committed entries after the last checkpoint are replayed
  (page-image redo), then the log checkpoints and truncates. Uncommitted
  work is never replayed. `node_count`/`edge_count` are allocation counters
  and are never decremented on delete.
- LSN stamping is layout-aware: regular pages stamp the page header, page 0
  stamps the meta header (`Pager::stamp_page_lsn`). Stamping page 0 as a
  regular page destroys the magic bytes — a real bug this rule once caught
  (see the meta-magic regression test in `db/inspect_test.rs`). Recovery
  extracts each page's LSN with the matching layout for the same reason.
- Read-only snapshots (`HiveDb::open_snapshot`, §19) run the same recovery
  redo, but the pager is in read-only mode (`Pager::open_read_only`):
  `write_page_to_disk` redirects into the private cache, dirty evictions are
  dropped instead of flushed, allocation/checkpoint/sync are refused or
  no-ops — so redo can never reach the shared files.

---

## 8. B-tree indexes, catalog, and unique constraints

Indexes are durable B+trees inside `hive.db` (`core/storage/btree/`), with
leaf/interior split, root growth, exact lookup, delete, and range scan.
`Transaction::btree_insert` / `btree_delete` capture before-images of all
touched pages and track new pages, so index writes are atomic with the data
write. An **index catalog** — itself a B+tree keyed by
`(entity_kind, label_id, property_key_id)` — maps each index to its root
page (`core/storage/index_catalog.rs`).

Available indexes (`HiveDb`, `core/db/hive_db.rs:295`):

```rust
db.create_node_label_index("Person")?;
db.create_edge_type_index("KNOWS")?;
db.create_node_property_index(Some("Person"), "age")?; // per-label
db.create_node_property_index(None, "age")?;            // global
db.create_edge_property_index(Some("KNOWS"), "since")?;
db.create_unique_constraint("Person", "email")?;
```

- The planner derives `NodeIndexHint::{FullScan, Label, Property,
  LabelAndProperty}` from label/property equality; the executor uses a hint
  when a matching index exists and otherwise full-scans. Indexed and
  full-scan plans return identical results.
- Index maintenance runs on create/set/delete **and** on label/property
  `REMOVE` (`Transaction::add_node_label`, `remove_node_label`,
  `remove_node_property`, `remove_edge_property`), inside the same
  transaction as the mutation.
- Unique constraints on `(label, property_key)` are backed by a per-label
  property index and enforced on create/set/merge; conflicts surface
  `DbError`, and the executor checks **all** labels of a node
  (`check_unique_constraint_multi`).

---

## 9. Query pipeline: parse → plan → execute

```text
Cypher string → Lexer → recursive-descent Parser → Statement (clauses)
  → Planner (scope check + QueryPlan steps + index hints)
  → Executor (binding rows in one Transaction) → QueryResult
```

- **Parser** (`parser/`): hand-written lexer + recursive-descent parser,
  `miette` diagnostics. Clauses: `CREATE`, `MATCH`, `OPTIONAL MATCH`,
  `WITH`, `WHERE`, `SET`, `DELETE`/`DETACH DELETE`, `REMOVE`, `MERGE` +
  `ON CREATE SET` / `ON MATCH SET`, `RETURN` (+ `ORDER BY`/`SKIP`/`LIMIT`).
- **Planner** (`core/query/planner.rs`): validates variable scope per
  clause and emits the `QueryPlan` enum —
  `CreateNode`, `CreateRelationship`, `MergeNode`, `MergeRelationship`,
  `ScanNodes`, `TraverseEdges`, `Filter`, `With`, `SetProperty`, `Delete`,
  `Remove`, `Return`, `Sequence` — plus
  `QueryPlan::is_read_only()` for the WAL-free commit path.
- **Executor** (`core/query/executor.rs`): evaluates plan steps as a stream
  of rows, where `type Row = HashMap<String, Binding>` and

```rust
enum Binding {
    Node(NodeId),   // packed record ID
    Edge(EdgeId),
    Value(Value),   // WITH projections, literals, params
}
```

`execute` delegates to `execute_with_params` with an empty map; both run
the whole plan in one transaction and roll back on any error
(`core/query/executor.rs:37`):

```rust
pub fn execute(plan: &QueryPlan, db: &mut HiveDb) -> Result<QueryResult, DbError> {
    execute_with_params(plan, db, &HashMap::new())
}
```

---

## 10. Core clauses: CREATE, MATCH, WHERE, SET, DELETE

- `CREATE (n:Label {k: v})` / single-segment relationship paths
  (`create_nodes`, `create_relationships`). Bound variables in the row are
  reused, so `MATCH … MATCH … CREATE (a)-[:T]->(b)` connects existing nodes.
- `MATCH` binds via `ScanNodes` (index hint or full scan, plus inline
  `{k: v}` filters) and `TraverseEdges` over adjacency chains with
  direction/type/label filtering. Re-mentioning a bound variable constrains
  rather than rebinds (`binding_matches`).
- `WHERE` filters rows on `eval_truthy` (`=`, `<>`, `>`, `>=`, `<`, `<=`,
  `AND`, `OR`, `NOT`); missing properties evaluate to `NULL`.
- `SET n.k = expr` evaluates per row and writes through the transaction
  (unique constraints checked first). `SET` on a `WITH`-projected value is
  rejected — only entities are writable.
- `DELETE n [, …]` collects distinct entity IDs across rows and deletes
  edges before nodes; plain node delete with incident edges errors (use
  `DETACH DELETE`, which gathers incident edges first). Failures roll back.

---

## 11. Production-safe MERGE

`MERGE` is match-or-create with deterministic identity, never blind insert:

- **Node MERGE** (`merge_nodes`, `core/query/executor.rs:348`): scans for a
  node with the same label + inline properties. On match it rebinds and
  runs `ON MATCH SET`; otherwise it creates (primary + extra labels,
  properties with constraint checks) and runs `ON CREATE SET`.
- **Relationship MERGE** (`merge_relationships`, `:396`): endpoints resolve
  with match-or-create semantics (`merge_get_node`), then the source's
  **outgoing chain** is searched for an edge with the same destination, type,
  and inline properties (respecting an already-bound edge variable). Match →
  `ON MATCH SET`; else create + `ON CREATE SET`.
- Parser/planner: `ON CREATE SET` / `ON MATCH SET` are separate clauses
  attached to the preceding `MERGE` (`MergeNode` / `MergeRelationship`
  carry `on_create: Vec<SetClause>, on_match: Vec<SetClause>`); a stray `ON`
  without `MERGE` is a plan error, and variable-length relationship `MERGE`
  is rejected.

```cypher
MERGE (n:Person {email: "a@b.com"})
ON CREATE SET n.created = true
ON MATCH SET n.visits = 2
MERGE (a:Person {name: "A"})-[:KNOWS]->(b:Person {name: "B"})
```

---

## 12. Query parameters

`$name` placeholders work anywhere an expression is allowed (`CREATE`,
`MATCH`, `WHERE`, `SET`, `MERGE`, property maps). The parser produces
`Expression::Param(name)`; the executor resolves it against a caller map,
and a missing key is a query error (`missing query parameter \`$name\``):

```rust
use std::collections::HashMap;
use hive::{HiveDb, Value};

let mut params = HashMap::new();
params.insert("name".to_string(), Value::String("Alice".into()));
let rows = db.execute_with_params(
    "MATCH (n:Person) WHERE n.name = $name RETURN n",
    &params,
)?;
```

(`HiveDb::execute_with_params`, `core/db/hive_db.rs:1038`;
`SharedDb::execute_with_params` for threaded use.)

---

## 13. WITH pipeline clause

`WITH` uses `RETURN`-style projection syntax and **replaces the visible
scope** with its aliases — variables not projected become plan errors
downstream. The executor (`project_with`) evaluates each item per row into a
fresh map: entity variables pass through as entity bindings (so a later
`MATCH` can traverse from them), computed values become `Binding::Value`.
`ORDER BY`/`SKIP`/`LIMIT` apply inside `WITH` like `RETURN`. It is a
read-only plan step. Aggregating `WITH` (`project_with_aggregate`) groups
rows exactly like `RETURN` aggregation and binds one row per group — entity
variables then evaluate to value maps, since a group has no single entity
to pass through.

```cypher
MATCH (a:Person) WITH a AS x MATCH (x)-[:KNOWS]->(b) RETURN b.name
MATCH (n:Person) WITH COUNT(*) AS c RETURN c
```

---

## 14. Aggregation: COUNT, SUM, AVG, MIN, MAX

`COUNT(*)` / `COUNT(expr)` parse to `Expression::Count(CountTarget::{Star,
Expr})`; `SUM` / `AVG` / `MIN` / `MAX` (all case-insensitive, `*` rejected)
parse to `Expression::{Sum, Avg, Min, Max}(Box<Expression>)`. Any aggregate
in `RETURN` (or `WITH`) switches to grouped mode: rows are grouped by the
evaluated non-aggregate expressions (`group_rows_for_aggregation`), each
group evaluates via `eval_aggregate_expr`, and ungrouped queries over empty
input still yield one row. `ORDER BY`/`SKIP`/`LIMIT` apply after
aggregation. Aggregates anywhere else (e.g. `WHERE`) are runtime errors.

```cypher
MATCH (n:Person) RETURN COUNT(*)
MATCH (n:Person) RETURN n.city, COUNT(n.city)
MATCH (n:Person) RETURN SUM(n.age), AVG(n.age), MIN(n.age), MAX(n.age)
MATCH (n:Person) WITH SUM(n.age) AS total RETURN total
```

Type and `NULL` rules (`eval_sum_agg`, `eval_avg_agg`, `eval_min_max_agg`):

- Every function skips `NULL`s. Empty/all-`NULL` groups yield `NULL` —
  except `COUNT(*)`, which yields `0`.
- `SUM`: integers accumulate to `Integer` (overflow via `checked_add` is an
  error); any float widens the result to `Float`. Non-numeric values error.
- `AVG`: always yields `Float`. Non-numeric values error.
- `MIN`/`MAX`: numbers (mixed int/float compare numerically) or strings
  (lexicographic) — one kind per group; mixed kinds, booleans, maps, and
  lists error. The winning original value is returned (no coercion).

---

## 15. OPTIONAL MATCH

`OPTIONAL MATCH` plans `ScanNodes`/`TraverseEdges` with `optional: true`.
The executor keeps the input row unchanged when nothing matches, so the new
variables stay unbound and read as `NULL` — a row-preserving outer join:

```cypher
MATCH (n:Person) OPTIONAL MATCH (n)-[:KNOWS]->(m) RETURN n.name, m.name
```

---

## 16. REMOVE and multiple labels

Nodes store one primary `label_id` plus `extra_labels: Vec<u32>`, so
`(n:Person:Employee)` parses into `label` + `extra_labels` and matches
require **all** requested labels (`node_matches`, `NodeRecord::has_label`).
`RETURN n` maps include a `labels` list next to `label`.

```cypher
CREATE (n:Person:Employee {name: "A"})
MATCH (n:Person) REMOVE n:Employee
MATCH (n:Person) REMOVE n.age
```

`REMOVE n:Label [, …]` / `REMOVE n.prop [, …]` parse to `RemoveClause` →
`QueryPlan::Remove` → `remove_entities`, which calls the transactional
`Transaction::{remove_node_label, remove_node_property,
remove_edge_property}`. Those update label and per-label/global property
indexes in the same transaction (removing an edge label is rejected).
Primary-label removal promotes the first extra label to keep the record
layout stable.

---

## 17. Variable-length traversal

Bounds `*`, `*min..max`, `*min..`, `*..max` parse to `RelationshipLength`
and ride `TraverseEdges.hops` into `traverse_edges_var_length`, which
BFS-expands adjacency chains honoring direction, edge type, and destination
labels:

```rust
/// Maximum hops explored for unbounded variable-length traversals.
pub const MAX_VAR_HOPS: u32 = 8;
```

Defaults are `min 1`; unbounded maxima cap at 8. A per-start-node visited
set gives cycle safety (each reachable node yields one row); edge variables
stay unbound (`NULL`) on multi-hop traversals.

```cypher
MATCH (a)-[:KNOWS*1..3]->(b) RETURN b
MATCH (a)-[:KNOWS*]->(b) RETURN b
```

---

## 18. RETURN shape, ordering, literals, comparison

- `RETURN n` / `RETURN r` build `Value::Map`s (`entity_to_map_for_node` /
  `entity_to_map_for_edge`): node → `{id, label, labels, properties}`;
  edge → `{id, type, src, dst, properties}`. Aliases via `AS`; default
  column names from `expression_name`.
- `ORDER BY … ASC/DESC`, `SKIP n`, `LIMIT n` apply in `RETURN` and `WITH`;
  `NULL` sorts before every other value. Cross-type numeric comparison
  (`Integer` vs `Float`) is supported in `compare_values`.
- Literals: `NULL`, integers including negatives (`-7`, parsed as unary
  minus), floats, booleans, double-quoted strings with escapes. Keywords
  such as `count` remain usable as property/label names.

---

## 19. Concurrency with SharedDb

`SharedDb` (`core/db/shared.rs`) pairs a primary `Arc<Mutex<HiveDb>>` with
an admission `RwLock`: writers take it exclusively, readers share it. One
rule governs it: admission first, then the inner mutex, once per operation —
never nest calls. Poisoned locks surface as `DbError::QueryError`.

```rust
use hive::SharedDb;

let db = SharedDb::open(std::path::Path::new("./.hive"))?;
db.execute("CREATE (n:Person {name: \"Alice\"})")?;
let result = db.execute_read("MATCH (n:Person) RETURN n.name AS name")?;
// Escape hatch for atomic multi-statement batches:
db.with_write(|db| db.execute("CREATE (n:Person)").map(|_| ()))?;
```

A mutex (not `RwLock`) guards the primary handle because the engine takes
`&mut` everywhere and `File` is `Send` but not `Sync`. True concurrent reads
come from **read-only snapshots** instead: `execute_read` opens a private
`HiveDb::open_snapshot` under the shared admission guard.

`open_snapshot` gets its own file handles and cache, replays committed WAL
entries into that cache only (the pager's disk-write paths redirect or
refuse in read-only mode: dirty evictions are dropped, allocation and
checkpoints fail loudly), and never touches the shared files — writers stay
excluded for the snapshot's lifetime, readers never block each other. Each
snapshot observes committed state as of its own open (unknown
labels/property keys resolve to `TRANSIENT_ID = u32::MAX`, matching
nothing, instead of being registered). `execute_read` rejects non-read-only
plans; handles from `from_db` (no known directory) fail snapshot reads
loudly. A pooled snapshot cache is future work.

---

## 20. Observability: EXPLAIN, stats, checkers, inspectors, metrics

```rust
println!("{}", db.explain("MATCH (a:Person)-[:KNOWS]->(b) RETURN a, b")?);
println!("{}", db.stats()?);                       // DbStats Display
assert!(db.check_integrity()?.is_empty());
assert!(db.check_index_consistency()?.is_empty());
println!("{}", db.inspect_page(0)?);                // meta page report
for entry in db.inspect_wal(Some(5))? { println!("{entry}"); }
let (result, metrics) = db.execute_timed("MATCH (n) RETURN n")?;
println!("{metrics}"); // "N rows in Xms (parse .., plan .., execute ..)"
```

- `HiveDb::explain` renders the planned steps without executing
  (`executor::explain_plan`).
- `HiveDb::stats` reports page-type counts, live node/edge counts, and
  metadata counters (`DbStats`).
- `check_integrity` verifies edge endpoints are live and adjacency chains
  agree with full scans; `check_index_consistency` walks the catalog and
  verifies every index entry points at a live record.
- `HiveDb::inspect_page(id)` returns a `PageReport` (header, slot
  live/dead counts, checksum verdict, LSN, plus decoded node/edge records,
  dictionary entries, B-tree cells with keys, freelist entries, or meta
  fields — undecodable slots are reported, never fatal). Out-of-range IDs
  are query errors.
- `HiveDb::inspect_wal(limit)` returns `WalEntrySummary` rows oldest-first
  (`limit` keeps the most recent entries with original indices).
- `HiveDb::execute_timed` / `execute_with_params_timed` return
  `(QueryResult, QueryMetrics)` with separate parse/plan/execute wall times
  plus row count (`QueryMetrics` is in the `hive::prelude`).
- The CLI exposes `.explain <query>`, `.stats`, `.check`,
  `.inspect <page>`, `.wal [limit]`, prints `QueryMetrics` after every
  query, and `SharedDb` forwards the timed variants.

---

## 21. Public API surface

`HiveDb` (`core/db/hive_db.rs`) — open/close, `open_snapshot`,
`is_snapshot`, `execute`, `execute_with_params`, `execute_timed`,
`execute_with_params_timed`, `explain`, `stats`, `check_integrity`,
`check_index_consistency`, `inspect_page`, `inspect_wal`, `begin`,
label/property-key registration and lookup, node/edge create/get/scan/delete,
property get/set/remove, label add/remove, index + constraint creation,
`checkpoint`. `Transaction` (`core/transaction.rs`) mirrors the mutating
surface with `commit`, `commit_readonly`, `rollback`, plus B-tree and
catalog helpers. `SharedDb` (`core/db/shared.rs`) offers `open`, `from_db`,
`execute`, `execute_with_params`, `execute_timed`,
`execute_with_params_timed`, `execute_read`, `stats`, both checkers, index /
constraint creation, and `with_write` / `with_read`. Errors are `DbError`
(`core/errors.rs`); values are `Value` (`core/value.rs`); timing is
`QueryMetrics` (in the `hive::prelude`).

---

## 22. Testing guide

```bash
cargo test --workspace                        # full suite (477 tests)
cargo test -p hive_core_testing               # engine/query tests only
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo check --workspace --all-targets
cargo doc --workspace --no-deps
```

Test layout (`testing/rust/core/`): `parser/` (10 files incl.
`advanced_clauses.rs` — now with `SUM`/`AVG`/`MIN`/`MAX` parse tests),
`query/planner_test.rs`, `query/executor_test.rs`,
`query/cypher_features_test.rs` (MERGE, params, `WITH`, `COUNT` +
`SUM`/`AVG`/`MIN`/`MAX` incl. `WITH` aggregates, `OPTIONAL MATCH`,
`REMOVE`/multi-label, variable-length, observability), `query/integration_test.rs`,
`db/` (node, edge, property, WAL, freelist, label, `concurrency_test.rs`
with snapshot-isolation and hammer tests, `inspect_test.rs` for page/WAL
inspectors, metrics, and the meta-magic regression test), `btree_test.rs`,
`index_test.rs`, `constraint_test.rs`, `page_storage/`.

When adding a feature: parser test for the syntax, planner test for the
steps/scope rules, executor/feature test end-to-end (including rollback and
reopen/recovery where state is involved). Temp-directory helpers live in
`testing/rust/core/utils/`.

---

## 23. Pending roadmap (post-v0.1.0 candidates)

Release packaging and crates.io publishing are intentionally deferred (owner
will handle separately). All other roadmap items are implemented:

1. ~~Extended aggregation (`SUM` / `AVG` / `MIN` / `MAX`)~~ — done (§14).
2. ~~Finer-grained concurrency (snapshots for true concurrent readers)~~ —
   done (§19). Remaining opportunity: a pooled snapshot cache to avoid the
   per-query open cost (WAL replay + cold cache on every `execute_read`).
3. ~~Page/WAL inspector tools and query timing metrics~~ — done (§20).

---

## 24. How to add a feature

Follow the pipeline order — each layer depends only on earlier ones:

1. **Syntax** (`parser/`): lexer token → AST node → recursive-descent parse
   function. Keywords that double as property/label names must stay
   accepted by `expect_ident`-style helpers.
2. **Plan** (`core/query/planner.rs`): new `QueryPlan` variant, scope
   validation, `is_read_only` classification, index hints where applicable.
3. **Execute** (`core/query/executor.rs`): row-stream function over
   `&mut Transaction` + `Params`; keep entity/value `Binding` discipline.
4. **Storage/tx** (`core/transaction.rs`, `core/db/hive_db.rs`,
   `core/storage/…`): transactional state changes with before-images and
   index maintenance in the same transaction.
5. **Surface**: `HiveDb`/`SharedDb`/CLI wiring if the feature needs it.
6. **Tests + docs**: parser, planner, and end-to-end tests (rollback/reopen
   included); update `docs/cypher.md`, `docs/storage.md`,
   `docs/architecture.md`, and this file.
