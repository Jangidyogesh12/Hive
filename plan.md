# Hive — Development Plan

> A local-first, serverless, Cypher-compatible graph database built in Rust.

---

## Architecture Overview

```
  hive> MATCH (n:Person)-[:KNOWS]->(m) RETURN m.name
       │
       ▼
  ┌──────────────────────────────────────────┐
  │           CLI / REPL (src/bin/)          │  Week 8
  └──────────────┬───────────────────────────┘
                 │ Cypher string
  ┌──────────────▼───────────────────────────┐
  │     pest Parser  (cypher.pest)           │  Week 5
  │     "MATCH (n:Person)->(m)" → AST       │
  └──────────────┬───────────────────────────┘
                 │ AST structs
  ┌──────────────▼───────────────────────────┐
  │     Query Planner (src/query/)           │  Week 6
  │     AST → QueryPlan (steps to execute)   │
  └──────────────┬───────────────────────────┘
                 │ QueryPlan
  ┌──────────────▼───────────────────────────┐
  │     Query Executor (src/query/)          │  Week 6-7
  │     Executes plan via HiveDb API calls   │
  └──────────────┬───────────────────────────┘
                 │ method calls
  ┌──────────────▼───────────────────────────┐
  │     HiveDb (src/db.rs)                   │  Week 3-4
  │     create_node, create_edge,            │
  │     get_neighbors, set_property, etc.    │
  └──────────────┬───────────────────────────┘
                 │ record operations
  ┌──────────────▼───────────────────────────┐
  │     Storage Layer (src/store/)           │  Week 1-2 (mostly done)
  │     NodeStore, EdgeStore, PropertyStore  │
  │     LabelStore, IndexStore               │
  └──────────────┬───────────────────────────┘
                 │ file I/O
  ┌──────────────▼───────────────────────────┐
  │     Flat binary files (.hive)            │  (done)
  │     nodes.hive, edges.hive, props.hive   │
  └──────────────────────────────────────────┘
```

---

## Known Bugs (Fix First)

| Bug | File | Line | Description | Status |
|---|---|---|---|---|
| Serialization bug | `src/store/edge_record.rs` | 41 | `first_property` never serialized — `next_in_edge` written twice | **Fixed** |
| No public API | `src/lib.rs` | 1-3 | All modules are private (`mod` not `pub mod`) | **Fixed** |
| Incomplete error type | `src/errors.rs` | 1-7 | Missing `Debug`, `Display`, `Error` trait impls | **Fixed** |
| `.DS_Store` in repo | `.gitignore` | — | macOS metadata file committed, not gitignored | **Fixed** |

**All known bugs fixed.** Ready to proceed with Day 2.

---

## Week 1 — Fix Bugs, Properties & Labels (Storage Foundation)

| Day | Task | Details |
|---|---|---|
| 1 | ~~Fix edge_record bug~~ + ~~error handling~~ | ~~Fixed: `edge_record.rs:41` serialization bug~~. ~~Fixed: `#[derive(Debug)]`, `impl Display`, `impl Error` added to `DbError` in `errors.rs`~~. All Day 1 tasks complete |
| 2 | ~~Make modules public~~ + ~~write tests~~ | ~~Fixed: `mod` changed to `pub mod` in `lib.rs`~~. ~~Completed store tests in `src/tests/` for `NodeStore` and `EdgeStore`: open/create file, append→read, append ordering, update→read, and out-of-bounds read error paths~~. Used shared temp-file helpers in `src/tests/utils/utils.rs` (no `tempfile` crate needed) |
| 3 | ~~Implement `PropertyRecord`~~ | ~~Completed `src/store/property_record.rs` with 48-byte layout: `id(u64)`, `key_hash(u64)`, `value_type(u8)`, `value_inline([u8;15])`, `next_property(u64)`, `flags(u32)`, `reserved(u32)`~~. ~~Implemented `new()`, `to_bytes()`, `from_bytes()`, added field comments, exported via `src/store/mod.rs`, and added tests in `src/tests/record/property_record_test.rs`~~ |
| 4 | Implement `PropertyStore` + `StringStore` | **Progress:** `PropertyStore` implementation + tests are complete. Implemented/updated files: `src/store/property_store.rs`, `src/store/mod.rs`, `src/tests/property_store/mod.rs`, `src/tests/property_store/store_open_test.rs`, `src/tests/property_store/store_read_append_test.rs`, `src/tests/property_store/store_update_test.rs`, `src/tests/mod.rs`. Remaining: implement `src/store/string_store.rs` with tests |
| 5 | Implement `LabelStore` + add `label_id` to NodeRecord | Create `src/store/label_store.rs` with bidirectional mapping: `label_id(u32) ↔ label_string`. Methods: `get_or_create(label) -> u32`, `get_by_id(id) -> Option<&str>`. Change `NodeRecord.reserved: u32` to `label_id: u32` (no size change). Update `to_bytes/from_bytes`. Write tests |

**Week 1 Deliverable:** Complete storage layer — nodes, edges, properties, labels, strings — all tested.

---

## Week 2 — HiveDb Orchestrator + Core Graph Operations

| Day | Task | Details |
|---|---|---|
| 6 | Build `HiveDb` struct | Create `src/db.rs`. Struct holds `NodeStore`, `EdgeStore`, `PropertyStore`, `StringStore`, `LabelStore`. Implement `HiveDb::open(path)` — opens/creates all store files in directory. Implement `HiveDb::close(self)`. Store files: `nodes.hive`, `edges.hive`, `props.hive`, `strings.hive`, `labels.hive` |
| 7 | `create_node` + `get_node` | `create_node(label, props) -> NodeId` — resolve label, create NodeRecord, link property chain. `get_node(id) -> Node` — read record, resolve label string, walk property chain, return rich `Node` struct |
| 8 | `create_edge` + `get_edge` | `create_edge(src, dst, edge_type, props) -> EdgeId` — create EdgeRecord, update src's `first_out_edge` linked list head, update dst's `first_in_edge` linked list head, link properties. `get_edge(id) -> Edge` — same pattern as get_node |
| 9 | `Value` type + property helpers | Create `src/value.rs` with `Value` enum: `Null`, `Integer(i64)`, `Float(f64)`, `Boolean(bool)`, `String(String)`. Implement `to_inline_bytes()` and `from_bytes()`. Add `set_property()` and `get_property()` helpers on HiveDb |
| 10 | `delete_node` + `delete_edge` + `get_neighbors` | `delete_node(id)` — set DELETED flag. `delete_edge(id)` — unlink from src out-edge chain and dst in-edge chain, set DELETED flag. `get_out_neighbors(id) -> Vec<NodeId>` — walk out-edge list, collect dst ids. `get_in_neighbors(id) -> Vec<NodeId>` — walk in-edge list, collect src ids |

**Week 2 Deliverable:** Working programmatic Rust API for creating/querying a property graph.

---

## Week 3 — Free List, DbHeader, and Query Infrastructure Setup

| Day | Task | Details |
|---|---|---|
| 11 | Free list for node/edge reuse | On delete, add ID to free list. Store in memory, persist to `freelist.hive`. On create, reuse freed slots before appending. Prevents unbounded file growth |
| 12 | Integrate `DbHeader` | Write header to `meta.hive`: `magic: [H,I,V,E,0,0,0,1]`, `version: 1`, `node_count`, `edge_count`, `property_count`, `free_node_head`, `free_edge_head`. Validate magic+version on open. Update counts on every create/delete. Test: open → create → close → reopen → verify counts |
| 13 | Setup query module + add `pest` | Add `pest` + `pest_derive` to `Cargo.toml`. Create `src/query/` directory: `mod.rs`, `ast.rs`, `cypher.pest`, `parser.rs`, `planner.rs`, `executor.rs`, `types.rs`. Define AST enums and structs on paper |
| 14 | Write minimal Cypher grammar (pest) | Create `src/query/cypher.pest` with PEG grammar for minimal Cypher subset: `CREATE (n:Label {key: val})`, `MATCH (n:Label)-[e:TYPE]->(m) WHERE n.key = val RETURN n, m`, `DELETE`, `SET`. Grammar rules for variable names, labels, property maps, relationship patterns, WHERE, RETURN |
| 15 | Write AST structs | Define in `src/query/ast.rs`: `Statement` enum (Create, Match, Delete, Set), `Pattern`, `PatternElement` (Node, Relationship), `NodePattern`, `RelationshipPattern`, `WhereClause`, `ReturnClause`, `Expression`. Map out every struct for the minimal Cypher subset |

**Week 3 Deliverable:** Storage complete with free lists and header. Query module set up with pest grammar and AST types.

---

## Week 4 — Cypher Parser + Basic Query Execution

| Day | Task | Details |
|---|---|---|
| 16 | Implement parser — CREATE | Create `src/query/parser.rs`. Use `pest_derive` to generate parser. Convert pest `Pair` tokens to AST. Start with `CREATE (n:Person {name: "Alice"})`. Test: parse valid/invalid Cypher, verify AST |
| 17 | Extend parser — MATCH + WHERE + RETURN | Add MATCH: `MATCH (n:Person) RETURN n`. Add relationship patterns: `(n)-[:KNOWS]->(m)`. Add WHERE: `WHERE n.age > 25`. Add RETURN with property access: `RETURN n.name, m.age`. Test each clause type |
| 18 | Implement query planner | Create `src/query/planner.rs`. Define `QueryPlan` enum: `CreateNode`, `CreateEdge`, `ScanNodes`, `TraverseEdges`, `DeleteEntities`, `SetProperty`. Convert AST → QueryPlan. Simple direct translation, no optimization yet |
| 19 | Implement executor — CREATE + simple MATCH | Create `src/query/executor.rs`. `Executor` holds `&mut HiveDb`. `execute(plan) -> QueryResult`. `CreateNode` calls `HiveDb::create_node()`. `ScanNodes` scans by label, applies filter, returns rows. `QueryResult`: column names + rows of `Value`s |
| 20 | Implement executor — relationships + DELETE + SET | `TraverseEdges`: resolve variable → get neighbors → filter by edge type → produce rows. `DeleteEntities`: resolve variables, call delete. `SetProperty`: resolve variable, call set_property. End-to-end test: CREATE nodes + edge, MATCH with traversal |

**Week 4 Deliverable:** Working Cypher engine — CREATE, MATCH, SET, DELETE through Cypher strings.

---

## Week 5 — Traversal Algorithms + Advanced MATCH

| Day | Task | Details |
|---|---|---|
| 21 | Multi-hop traversal | Support variable-length paths: `(n)-[:KNOWS*1..3]->(m)` (1 to 3 hops). BFS-based traversal. Track visited nodes to prevent infinite loops in cyclic graphs |
| 22 | Bidirectional traversal + compound WHERE | Support `<-[:KNOWS]-` (incoming), `-[:KNOWS]-` (undirected). Multiple WHERE conditions: `WHERE n.age > 25 AND m.age < 40`. Expression evaluation: `=`, `>`, `<`, `>=`, `<=`, `<>`, AND/OR/NOT |
| 23 | Complex MATCH patterns | Multiple relationship patterns: `MATCH (a)-[:KNOWS]->(b)-[:WORKS_AT]->(c)`. Planner chains traversals — bind `a` from scan, traverse to `b`, traverse to `c`. Core graph pattern matching |
| 24 | MERGE clause | `MERGE (n:Person {name: "Alice"})` — find or create. If matching node exists, return it; otherwise create. Idempotent operations for data loading |
| 25 | RETURN expressions + formatting | Return full nodes: `RETURN n`. Return properties: `RETURN n.name, n.age`. Return edges: `RETURN e`. Alias support: `RETURN n.name AS person_name`. Format results as ASCII table |

**Week 5 Deliverable:** Cypher handles multi-hop traversals, complex patterns, MERGE, formatted output.

---

## Week 6 — Indexing for Query Performance

| Day | Task | Details |
|---|---|---|
| 26 | Design index architecture | In-memory indexes rebuilt on DB open. Three types: label index (`label_id → Vec<NodeId>`), property index (`(key_hash, value) → Vec<NodeId>`), edge type index (`edge_type_id → Vec<EdgeId>`). Create `src/index.rs` with `IndexStore` |
| 27 | Implement label index | `LabelIndex` — `HashMap<u32, Vec<NodeId>>`. Updated on `create_node`/`delete_node`. `lookup_by_label(id) -> &[NodeId]` returns candidates instantly. No more full file scan for labeled MATCH queries |
| 28 | Implement property index | `PropertyIndex` — `HashMap<(u64, Value), Vec<NodeId>>`. Updated on `set_property`/`delete_node`. For `WHERE n.age = 25`, lookup `(hash("age"), Integer(25))` → instant node list. Exact-match only for v1 |
| 29 | Integrate indexes into planner | Two strategies for `MATCH (n:Label) WHERE n.prop = val`: full scan vs index scan. Planner picks index scan when available. Benchmark: scan vs indexed query on 10,000 nodes |
| 30 | Index persistence + rebuild | Save indexes to `indexes.hive` on close. Load on open. `HiveDb::rebuild_indexes()` — full scan to reconstruct. Auto-rebuild if file missing/corrupted. Test: create 1000 nodes → close → reopen → verify indexes |

**Week 6 Deliverable:** Indexed queries that scale — O(1) lookups instead of full scans.

---

## Week 7 — WAL, Transactions, and Crash Recovery

| Day | Task | Details |
|---|---|---|
| 31 | Write-Ahead Log implementation | Create `src/wal.rs`. WAL format: `[length: u32][type: u8][payload: bytes][checksum: u32]`. Entry types: CreateNode, CreateEdge, UpdateNode, UpdateEdge, DeleteNode, DeleteEdge, Checkpoint. Write intent to WAL before every storage write |
| 32 | Checkpoint mechanism | Write `Checkpoint` entry after flushing indexes and stores. On open: if WAL ends with checkpoint → clean shutdown, truncate WAL. No checkpoint → crash detected. Test: force unclean shutdown, verify WAL state |
| 33 | Crash recovery | On `HiveDb::open`: (1) open stores, (2) check WAL for entries after last checkpoint, (3) replay each entry, (4) write checkpoint, truncate WAL. Test: insert → kill → reopen → verify data |
| 34 | Basic transactions | Create `src/transaction.rs`. `begin() -> Transaction`, `commit()` writes buffered ops to WAL atomically, `rollback()` discards. Single-writer model. Test: commit persists, rollback discards |
| 35 | Buffered I/O + benchmarking | Replace `write + flush` with `BufWriter`. Flush only on commit/checkpoint. Add `criterion` benchmarks: node insert, edge insert, 1-hop traversal, 3-hop traversal, indexed lookup vs scan. Run and identify bottlenecks |

**Week 7 Deliverable:** Durable database with crash recovery, transactions, and performance baseline.

---

## Week 8 — CLI REPL, Polish & Open Source Launch

| Day | Task | Details |
|---|---|---|
| 36 | Build CLI binary | Create `src/bin/hive.rs`. REPL with `hive> ` prompt. Commands: `:open <path>`, `:status`, `:exit`. Any Cypher query → parse + execute. Use `rustyline` for history and arrow keys |
| 37 | Result formatting + examples | Format results as ASCII tables with column headers. Create `examples/social_graph.rs` and `examples/knowledge_graph.rs` as programmatic API demos |
| 38 | Documentation | Rustdoc on all public types. Write README: what is Hive, architecture, quick start, supported Cypher subset with examples, storage format, project structure |
| 39 | CI + code quality | GitHub Actions: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`. Add `CONTRIBUTING.md`. Choose license (MIT or Apache 2.0). Add `.DS_Store` to `.gitignore`. Fix all clippy warnings |
| 40 | Polish + publish | Review public API ergonomics. Add `HiveDb::info()` for DB stats. Final README with REPL screenshots. Tag `v0.1.0`. Publish to crates.io |

**Week 8 Deliverable:** Documented, tested, open-source graph DB with Cypher — ready for v0.1.0.

---

## Summary — Build Order

| Week | Focus | Key Milestone |
|---|---|---|
| 1 | Storage (properties, labels, strings, fix bugs) | Foundation complete |
| 2 | HiveDb orchestrator (create/read/delete graph ops) | Usable Rust API |
| 3 | Free lists, header, query module setup + grammar | Prep for Cypher |
| 4 | Cypher parser + planner + executor (basic queries) | Cypher works! |
| 5 | Multi-hop traversal, MERGE, complex patterns | Real graph queries |
| 6 | Indexing (label, property, edge-type indexes) | Performance |
| 7 | WAL, crash recovery, transactions | Durability |
| 8 | CLI REPL, docs, CI, publish | Open source launch |

---

## Language Compatibility

| Version | Milestone |
|---|---|
| v0.1 | Rust crate only |
| v0.2 | C FFI (`src/ffi.rs`) — unlocks C, C++, Python, Go, Ruby, Swift, Zig, etc. |
| v0.3 | WASM compilation — unlocks browsers and JS/TS |
| Later | Language-specific SDKs (`hive-python`, `hive-node`) |

---

## Current Status

- [x] NodeRecord (data model + serialization)
- [x] NodeStore (file open/append/read/update)
- [x] EdgeRecord (data model + serialization)
- [x] EdgeStore (file open/append/read/update)
- [x] Type aliases (NodeId, EdgeId, PropertyId, NIL_ID)
- [x] Error enum (DbError) — Debug/Display/Error implemented
- [x] DbHeader struct — defined but unused
- [x] Fix edge_record serialization bug
- [x] Make modules public
- [x] Add .DS_Store to .gitignore
- [x] Complete DbError traits (Debug, Display, Error)
- [x] Write tests for existing stores
- [x] Add tests for record constructors + serialization roundtrip
- [x] Add file-level and test-level comments across `src/`
- [x] PropertyRecord (data model + serialization)
- [x] PropertyRecord tests (defaults + roundtrip)
- [x] PropertyStore (open/append/read/update implemented)
- [x] PropertyStore tests (open + append/read + update + out-of-bounds)
- [ ] StringStore
- [ ] LabelStore
- [ ] HiveDb orchestrator
- [ ] Value type
- [ ] Free list
- [ ] DbHeader integration
- [ ] pest grammar file
- [ ] AST structs
- [ ] Parser
- [ ] Query planner
- [ ] Query executor
- [ ] Traversal algorithms
- [ ] MERGE clause
- [ ] Index architecture
- [ ] Label index
- [ ] Property index
- [ ] Edge type index
- [ ] WAL
- [ ] Checkpoint
- [ ] Crash recovery
- [ ] Transactions
- [ ] CLI REPL
- [ ] Documentation
- [ ] CI/CD
- [ ] v0.1.0 release
