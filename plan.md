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
  │     HiveDb (src/db/hive_db.rs)           │  Week 3-4
  │     create_node, create_edge,            │
  │     get_neighbors, set_property, etc.    │
  └──────────────┬───────────────────────────┘
                 │ record operations
  ┌──────────────▼───────────────────────────┐
  │     Storage Layer (src/store/)           │  Week 1-2 (mostly done)
  │     NodeStore, EdgeStore, PropertyStore  │
  │     LabelStore                           │
  └──────────────┬───────────────────────────┘
                  │ file I/O
  ┌──────────────▼───────────────────────────┐
  │     Index Layer (src/db/index.rs)        │  Week 6
  │     Label / Property / EdgeType indexes  │
  └──────────────┬───────────────────────────┘
                 │ persisted cache
  ┌──────────────▼───────────────────────────┐
  │     Flat binary files (.hive)            │  (done)
  │ nodes.hive, edges.hive, props.hive,      │
  │ strings.hive, labels.hive, indexes.hive  │
  └──────────────────────────────────────────┘
```

---

## Known Bugs

All resolved in Week 1. No outstanding bugs.

---

## Week 1 — Fix Bugs, Properties & Labels (Storage Foundation)

| Day | Task | Details |
|---|---|---|
| 1 | ~~Fix edge_record bug~~ + ~~error handling~~ | ~~Fixed: `edge_record.rs:41` serialization bug~~. ~~Fixed: `#[derive(Debug)]`, `impl Display`, `impl Error` added to `DbError` in `errors.rs`~~. All Day 1 tasks complete |
| 2 | ~~Make modules public~~ + ~~write tests~~ | ~~Fixed: `mod` changed to `pub mod` in `lib.rs`~~. ~~Completed store tests in `src/tests/` for `NodeStore` and `EdgeStore`: open/create file, append→read, append ordering, update→read, and out-of-bounds read error paths~~. Used shared temp-file helpers in `src/tests/utils/utils.rs` (no `tempfile` crate needed) |
| 3 | ~~Implement `PropertyRecord`~~ | ~~Completed `src/store/property_record.rs` with 48-byte layout: `id(u64)`, `key_hash(u64)`, `value_type(u8)`, `value_inline([u8;15])`, `next_property(u64)`, `flags(u32)`, `reserved(u32)`~~. ~~Implemented `new()`, `to_bytes()`, `from_bytes()`, added field comments, exported via `src/store/mod.rs`, and added tests in `src/tests/record/property_record_test.rs`~~ |
| 4 | ~~Implement `PropertyStore` + `StringStore`~~ | ~~All complete. `PropertyStore`: `src/store/property_store.rs` + tests in `src/tests/property_store/`. `StringStore`: `src/store/string_store.rs` (append-only, length-prefixed, variable-length strings with offset-based read) + tests in `src/tests/string_store/`. Both exported in `src/store/mod.rs`. Fixed endianness bug in `StringStore` (append now uses `to_le_bytes` matching `from_le_bytes` in read). No record struct needed for StringStore (variable-length data)~~ |
| 5 | ~~Implement `LabelStore` + add `label_id` to NodeRecord~~ | ~~Completed `src/store/label_store.rs` with bidirectional mapping: `label_to_id` HashMap + `id_to_label` HashMap. Methods: `get_or_create(label) -> u32`, `get_by_id(id) -> Option<&str>`. Writes `[id (4B)][len (4B)][string bytes]` to `labels.hive`. Renamed `NodeRecord.reserved → label_id: u32` (no size change, still 40 bytes). Updated `to_bytes/from_bytes` and all test references. Tests in `src/tests/label_store/` (16 tests: open, get_or_create, get_by_id). All 50 tests passing~~ |

**Week 1 Deliverable:** Complete storage layer — nodes, edges, properties, labels, strings — all tested.

---

## Week 2 — HiveDb Orchestrator + Core Graph Operations

| Day | Task | Details |
|---|---|---|
| 6 | ~~Build `HiveDb` struct~~ | ~~Completed in `src/db/hive_db.rs` + `src/db/store_path.rs`. `HiveDb` holds `NodeStore`, `EdgeStore`, `PropertyStore`, `StringStore`, `LabelStore`. `ensure_db_dir()` uses `create_dir_all`. `DbError` implements `From<std::io::Error>`. `HiveDb::open(path)` constructs all store paths and opens all files: `nodes.hive`, `edges.hive`, `props.hive`, `strings.hive`, `labels.hive`. `HiveDb::close(self)` added.~~ |
| 7 | ~~`create_node` + `get_node`~~ | ~~`create_node(label, props) -> NodeId` — resolve label, create NodeRecord, link property chain. `get_node(id) -> Node` — read record, resolve label string, walk property chain, return rich `Node` struct. Tests in `src/tests/db/hive_db_test.rs`: create+get no props, single/multi prop, label dedup, out-of-bounds, persistence across reopen.~~ |
| 8 | ~~`create_edge` + `get_edge`~~ | ~~`create_edge(src, dst, edge_type, props) -> EdgeId` — create EdgeRecord, load label, link properties. `get_edge(id) -> Edge` — read record, resolve label, walk property chain. 9 tests: create/get with no/single/multi props, label dedup, out-of-bounds, persistence across reopen, edge+node coexistence, sequential IDs~~ |
| 9 | ~~`Value` type + property helpers~~ | ~~Create `src/value.rs` with `Value` enum: `Null`, `Integer(i64)`, `Float(f64)`, `Boolean(bool)`, `String(String)`. Implement `to_inline_bytes()` and `from_bytes()`. Add `set_node_property()`, `get_node_property()`, `set_edge_property()`, `get_edge_property()` helpers on HiveDb~~ |
| 10 | ~~`delete_node` + `delete_edge` + `get_neighbors`~~ | ~~`delete_node(id)` — set DELETED flag. `delete_edge(id)` — unlink from src out-edge chain and dst in-edge chain, set DELETED flag. `get_out_neighbors(id) -> Vec<NodeId>` — walk out-edge list, collect dst ids. `get_in_neighbors(id) -> Vec<NodeId>` — walk in-edge list, collect src ids~~ |

**Week 2 Deliverable:** Working programmatic Rust API for creating/querying a property graph. (Days 6-10 complete.)

---

## Week 3 — Free List, DbHeader, and Query Infrastructure Setup

| Day | Task | Details |
|---|---|---|
| 13 | ~~Setup query module + add `pest`~~ | ~~Added `pest` + `pest_derive` to `Cargo.toml`. Created `src/query/` directory: `mod.rs`, `ast.rs`, `cypher.pest`, `parser.rs`, `planner.rs`, `executor.rs`, `types.rs`.~~ |
| 14 | ~~Write minimal Cypher grammar (pest)~~ | ~~Created `src/query/cypher.pest` with PEG grammar: `CREATE (n:Label {key: val})`, `MATCH (n:Label)-[e:TYPE]->(m) WHERE n.key = val RETURN n, m`, `DELETE`, `SET`. Grammar rules for variables, labels, property maps, relationship patterns, WHERE, RETURN.~~ |
| 15 | ~~Write AST structs~~ | ~~Defined in `src/query/ast.rs`: `Statement` (Create, Match, Delete, Set), `Pattern` (Node/Path), `NodePattern`, `RelationshipPattern`, `Direction`, `WhereClause`, `ReturnClause`, `ReturnItem`, `Expression` with `BinaryOp`/`UnaryOp`.~~ |

**Week 3 Deliverable:** Free list + DbHeader complete. Query module setup done.

---

## Week 4 — Cypher Parser + Basic Query Execution

| Day | Task | Details |
|---|---|---|
| 16 | ~~Implement parser — CREATE~~ | ~~Created `src/query/parser.rs`. Uses `pest_derive` to generate parser. Converts pest `Pair` tokens to AST. Handles `CREATE (n:Person {name: "Alice"})`. `parse()` function + `build_statement()`, `build_node_pattern()`, `build_property_map()`, `build_expression()` helpers.~~ |
| 17 | ~~Extend parser — MATCH + WHERE + RETURN~~ | ~~Added MATCH: `MATCH (n:Person) RETURN n`. Added relationship patterns: `(n)-[:KNOWS]->(m)`. Added WHERE: `WHERE n.age > 25`. Added RETURN with property access: `RETURN n.name, m.age`. All rule types handled in `build_*` functions.~~ |
| 18 | ~~Implement query planner~~ | ~~Created `src/query/planner.rs`. `QueryPlan` enum: `CreateNode`, `ScanNodes`, `TraverseEdges`, `Filter`, `Return`, `DeleteEntity`, `SetProperty`, `Sequence`. `plan()` function converts AST → QueryPlan. `merge_conditions()`, `and_chain()` helpers.~~ |
| 19 | ~~Implement executor — CREATE + simple MATCH~~ | ~~Created `src/query/executor.rs`. `Executor` holds `&mut HiveDb`. `execute(plan) -> QueryResult`. `exec_create_node()` calls `HiveDb::create_node()`. `scan_nodes()` scans by label, applies filter, returns rows. `QueryResult`: columns + rows of `Value`s (`src/query/types.rs`).~~ |
| 20 | ~~Implement executor — relationships + DELETE + SET~~ | ~~`traverse_edges()`: resolve variable → walk edge chains → filter by type → produce rows. `walk_edges()` handles incoming/outgoing/undirected. `exec_delete()` + `exec_set_property()` resolve variables from bound rows. Expression evaluator with `eval_binary_op`, `cmp_values`, `eval_unary_op`.~~ |

**Week 4 Deliverable:** Working Cypher engine — CREATE, MATCH, SET, DELETE through Cypher strings. (Complete.)

---

## Week 5 — Traversal Algorithms + Advanced MATCH

| Day | Task | Details |
|---|---|---|
| 21 | ~~Multi-hop traversal~~ | ~~Completed variable-length path support in `src/query/executor.rs` via BFS queue (`VecDeque<(node_id, depth)>`). Added hop-range handling (`min_hops`/`max_hops`), depth cutoff (`if depth >= max_hops { continue; }`), and `visited`-set cycle protection to avoid infinite traversal in cyclic graphs.~~ |
| 22 | ~~Bidirectional traversal + compound WHERE~~ | ~~Completed support for incoming traversal `<-[:KNOWS]-` and undirected traversal `-[:KNOWS]-` through direction-aware planning/execution (`Direction::Incoming`, `Direction::Undirected`) and edge walking in executor. Completed compound WHERE support with chained boolean/comparison expressions: `=`, `>`, `<`, `>=`, `<=`, `<>`, `AND`, `OR`, `NOT`. Updated grammar precedence in `src/query/cypher.pest` (`NOT` > `AND` > `OR`) and added parser handling for unary NOT in `src/query/parser.rs`. Added parser and E2E coverage for incoming/undirected traversal plus NOT/comparison combinations in `src/tests/query/match_test.rs` and `src/tests/query/e2e_test.rs`.~~ |
| 23 | ~~Complex MATCH patterns~~ | ~~Completed support for chained relationship patterns in MATCH, including multi-segment parsing/planning/execution flow (e.g., `MATCH (a)-[:KNOWS]->(b)-[:WORKS_AT]->(c)`). Updated grammar (`pattern = node_pattern ~ (rel_pattern ~ node_pattern)*`), parser path construction (`PathPattern { start, segments }`), planner traversal chaining across segments, and parser/E2E tests for complex path matching in `src/tests/query/match_test.rs` and `src/tests/query/e2e_test.rs`.~~ |
| 24 | ~~MERGE clause~~ | ~~Implemented single-node MERGE find-or-create flow (`MERGE (n:Person {name: "Alice"})`) with idempotent behavior in planner/executor. Added parser support, `QueryPlan::MergeNode`, executor lookup-by-label+properties before create, and tests (`src/tests/query/merge_test.rs`, MERGE E2E cases in `src/tests/query/e2e_test.rs`) including path-merge rejection for now.~~ |
| 25 | ~~RETURN expressions + formatting~~ | ~~Completed RETURN projection output flow in query executor and result renderer. Supports full bindings (`RETURN n`, `RETURN e`), property projections (`RETURN n.name, n.age`), aliases (`RETURN n.name AS person_name`), and ASCII table formatting via `QueryResult::to_ascii_table()` in `src/query/result.rs`. Covered by parser/executor tests in `src/tests/query/`.~~ |

**Week 5 Deliverable:** Cypher handles multi-hop traversals, complex patterns, MERGE, formatted output.

---

## Week 6 — Indexing for Query Performance

| Day | Task | Details |
|---|---|---|
| 26 | ~~Design index architecture~~ | ~~Completed in `src/db/index.rs`. `IndexStore` owns three persisted/rebuildable indexes: label index (`label_id -> Vec<NodeId>`), property equality index (`(key_hash, normalized_value) -> Vec<NodeId>`), and edge type index (`edge_type_id -> Vec<EdgeId>`). Added `indexes.hive` to `src/db/store_path.rs`.~~ |
| 27 | ~~Implement label index~~ | ~~Implemented `label_index: HashMap<u32, Vec<NodeId>>` with rebuild, load/save, and mutation maintenance on node create/delete. Added `HiveDb::lookup_node_ids_by_label()` and label reload support in `LabelStore::open()`.~~ |
| 28 | ~~Implement property index~~ | ~~Implemented exact-match node property index using `PropertyIndexKey { key_hash, value }` and normalized `IndexValue` (`Null`, `Integer`, `FloatBits`, `Boolean`, `String`). Maintained on node property set/update/delete. Long strings are normalized through `StringStore` reads during rebuild.~~ |
| 29 | ~~Integrate indexes into executor~~ | ~~Implemented index-aware node scan candidate selection in `src/query/executor.rs`. `MATCH` node scans now prefer exact property equality lookup, label lookup, or intersection of both when available, and still fall back to full scan for non-indexable predicates.~~ |
| 30 | ~~Index persistence + rebuild~~ | ~~Implemented `IndexStore::load_or_rebuild()` path in `HiveDb::open()`. `indexes.hive` is loaded when valid, otherwise rebuilt from source-of-truth stores and saved back. Added tests for persisted index reopen, delete cleanup, and property updates.~~ |

**Week 6 Deliverable:** Indexed queries that scale — O(1) lookups instead of full scans.

---

## Week 7 — WAL, Transactions, and Crash Recovery

| Day | Task | Details |
|---|---|---|
| 31 | ~~Write-Ahead Log implementation~~ | ~~Completed `src/wal.rs` with WAL file API (`Wal::open`, `append`, `read_all`, `sync`, `truncate`), logical entry types (`CreateNode`, `CreateEdge`, `UpdateNode`, `UpdateEdge`, `DeleteNode`, `DeleteEdge`, `Checkpoint`), binary format `[length: u32][type: u8][payload: bytes][checksum: u32]`, and CRC32 over `type + payload`. Integrated WAL intent logging into `HiveDb` mutators in `src/db/hive_db.rs` so `create_node`, `create_edge`, `set_node_property`, `set_edge_property`, `delete_node`, and `delete_edge` append and `sync()` WAL entries before storage writes. Added standalone WAL tests plus `HiveDb` integration tests in `src/tests/db/wal_test.rs`.~~ |
| 32 | ~~Checkpoint mechanism~~ | ~~Completed checkpoint integration in `src/db/hive_db.rs`: successful mutators now append `WalEntry::Checkpoint` after durable writes, and `HiveDb::open()` now detects a clean WAL state (empty or ending in checkpoint) and truncates it on reopen. Added end-to-end coverage in `src/tests/db/wal_test.rs` verifying checkpointed WAL is cleared after reopen while dirty WAL is preserved for future recovery work.~~ |
| 33 | Crash recovery | Design still pending `HiveDb` integration. Current WAL can parse a valid prefix and ignore corrupt or partial trailing entries, which is the foundation for recovery. Remaining work: open WAL in `HiveDb::open()`, inspect entries after the last checkpoint, replay uncheckpointed operations safely, then checkpoint and truncate. Special care is needed for pointer-mutating operations like `create_edge`, `set_*_property`, and `delete_edge`, where mid-write crashes can leave partial graph-link updates behind. |
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

- [x] NodeRecord (data model + serialization) — `src/store/node/record.rs`
- [x] NodeStore (file open/append/read/update) — `src/store/node/store.rs`
- [x] EdgeRecord (data model + serialization) — `src/store/edge/record.rs`
- [x] EdgeStore (file open/append/read/update) — `src/store/edge/store.rs`
- [x] Type aliases (NodeId, EdgeId, PropertyId, NIL_ID) — `src/types.rs`
- [x] Error enum (DbError) — Debug/Display/Error implemented
- [x] DbHeader struct — `src/store/header.rs` (52-byte layout, magic bytes, version, counts, serialization, file I/O)
- [x] Fix edge_record serialization bug
- [x] Make modules public (`pub mod` in `src/lib.rs`)
- [x] Add .DS_Store to .gitignore
- [x] Store tests — `src/tests/node_store/`, `src/tests/edge_store/`, `src/tests/property_store/`
- [x] Record tests — `src/tests/record/` (node, edge, property roundtrips)
- [x] Shared test utilities — `src/tests/utils/utils.rs` (temp_dir, temp_file, cleanup)
- [x] PropertyRecord (data model + 48-byte serialization) — `src/store/property/record.rs`
- [x] PropertyStore (open/append/read/update) — `src/store/property/store.rs`
- [x] StringStore (append-only, length-prefixed variable-length strings) — `src/store/string_store.rs`
- [x] StringStore tests — `src/tests/string_store/`
- [x] LabelStore (bidirectional label→id mapping) — `src/store/label_store.rs`
- [x] LabelStore tests — `src/tests/label_store/`
- [x] HiveDb orchestrator — `src/db/hive_db.rs` (Day 6-7)
- [x] HiveDb::open / HiveDb::close
- [x] HiveDb::create_node (label + property chain linking)
- [x] HiveDb::get_node (record read + label resolution + property chain walk)
- [x] HiveDb tests — `src/tests/db/` (refactored from single file into `delete_test.rs`, `edge_test.rs`, `header_test.rs`, `neighbors_test.rs`, `node_test.rs`, `property_test.rs`)
- [x] HiveDb::create_edge (label + property chain linking) + get_edge (record read + label resolution + property chain walk)
- [x] Value type (`src/value.rs` with Null, Integer, Float, Boolean, String, to_inline_bytes, from_bytes)
- [x] set_property / get_property helpers (set_node_property, get_node_property, set_edge_property, get_edge_property)
- [x] delete_node / delete_edge (set DELETED flag, unlink edge chains)
- [x] get_neighbors (out/in edge traversal, skip deleted)
- [x] Free list — `src/store/free_list.rs` (push/pop/flush/persistence, integrated in HiveDb)
- [x] DbHeader integration
- [x] DbHeader::SIZE (52 bytes), to_bytes / from_bytes serialization
- [x] HIVE_MAGIC constant — `[b'H', b'I', b'V', b'E', 0, 0, 0, 1]`
- [x] CURRENT_VERSION constant (1) — checked on open
- [x] read_header / write_header — meta.hive file I/O with magic validation
- [x] InvalidHeader / UnsupportedVersion error variants
- [x] Counter updates on every create/delete — node_count, edge_count, property_count
- [x] Double-delete guard — prevents node_count / edge_count underflow on idempotent delete
- [x] META_FILE constant — `"meta.hive"` in store_path.rs
- [x] flush_header() helper — writes header to disk after every mutation
- [x] pest grammar file (`src/query/cypher.pest` — CREATE, MATCH, DELETE, SET, WHERE, RETURN, expressions, comparisons, literals)
- [x] AST structs (`src/query/ast.rs` — Statement, Pattern, NodePattern, RelationshipPattern, Direction, Expression, BinaryOp, UnaryOp, etc.)
- [x] Parser (`src/query/parser.rs` — pest-based Cypher parser with full expression support)
- [x] Query planner (`src/query/planner.rs` — AST → QueryPlan translation, merge_conditions)
- [x] Query executor (`src/query/executor.rs` — Executor with execute(), scan_nodes(), traverse_edges(), expression eval, RETURN formatting)
- [x] QueryResult type (`src/query/result.rs` — columns + rows of Values + ASCII table formatting)
- [x] Query E2E tests — `src/tests/query/` (`create_test.rs`, `match_test.rs`, `delete_test.rs`, `set_test.rs`, `e2e_test.rs`)
- [x] Traversal algorithms (multi-hop, variable-length paths) — `src/query/executor.rs` (`traverse_edges` with BFS, hop limits, visited-set cycle protection)
- [x] Bidirectional traversal + compound WHERE — incoming/undirected relationship traversal, NOT precedence, and expression coverage (`=`, `>`, `<`, `>=`, `<=`, `<>`, `AND`, `OR`, `NOT`) with tests in `src/tests/query/match_test.rs` and `src/tests/query/e2e_test.rs`
- [x] Complex MATCH patterns — chained relationship path matching (e.g., `(a)-[:KNOWS]->(b)-[:WORKS_AT]->(c)`), with parser/planner support and query parser/E2E coverage
- [x] MERGE clause
- [x] RETURN expressions + formatting — aliases, full bindings, property projections, ASCII table output
- [x] Index architecture — `src/db/index.rs` (`IndexStore`, `IndexValue`, `PropertyIndexKey`, load/save/rebuild flow)
- [x] Label index — persisted + rebuilt + maintained on node create/delete
- [x] Property index — exact-match node property index with normalized values and long-string support
- [x] Edge type index — persisted + rebuilt + maintained on edge create/delete
- [x] Index persistence + rebuild — `indexes.hive` load-or-rebuild path in `HiveDb::open()`
- [x] Basic index-assisted query execution — label/property candidate selection in `src/query/executor.rs`
- [ ] Edge property index
- [ ] Smarter planner-level index selection
- [ ] Benchmarks for indexed lookup vs scan
- [ ] WAL
- [x] Checkpoint
- [ ] Crash recovery
- [ ] Transactions
- [ ] CLI REPL
- [ ] Documentation
- [ ] CI/CD
- [ ] v0.1.0 release
