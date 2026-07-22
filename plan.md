# Hive Production Graph Database Plan

> Source: `Study.md`, current codebase state, and the goal of executing production-grade graph queries end-to-end.

---

## Current Baseline

Hive already has a strong storage foundation:

- Page-based storage with 4KB pages, page headers, slotted records, checksums, and meta page.
- Packed `NodeId`, `EdgeId`, and reserved flags.
- Buffer pool and page cache with dirty/spilled state and SIEVE eviction.
- Pager with page allocation, read/write, WAL LSN stamping, recovery, checkpointing, and rollback before-images.
- Node, edge, property, label, and overflow string storage.
- WAL-backed autocommit and explicit transaction APIs.
- Parser, planner, and query executor for a subset of Cypher-like statements.
- Public `HiveDb::execute(query)` API exists for parse-plan-execute flow.

The next goal is to evolve Hive from a storage engine with basic CRUD into a production-grade graph database that can parse, plan, execute, recover, and optimize real graph queries.

---

## Product Goal

Hive should support durable, transactional, indexed graph workloads with a Cypher-like query surface:

```cypher
CREATE (a:Person {name: "Alice", age: 30})
CREATE (a)-[:KNOWS {since: 2020}]->(b)
MATCH (a:Person)-[r:KNOWS]->(b:Person)
WHERE a.age > 25
SET b.active = true
DELETE r
MERGE (p:Person {email: "a@example.com"})
RETURN a.name AS name, b.age AS age
```

Production-grade means:

- Queries are atomic, durable, and recoverable.
- Mutating queries rollback fully on error.
- Node and edge deletes preserve graph integrity.
- `MERGE` is deterministic and eventually backed by uniqueness/index constraints.
- Scans work first, indexes are added without changing query semantics.
- Tests cover parser, planner, executor, storage, WAL recovery, indexes, and crash-style behavior.

## Production Implementation Policy

No phase is production-complete while any required subtask, dependency, rollback path, recovery path, or test category is missing.

Status meanings:
- Production Complete: all tasks, dependencies, tests, and recovery guarantees for the phase are done.
- Foundation Implemented: useful code exists, but the phase is blocked by missing production requirements.
- Blocked: do not implement this phase before its dependencies are production complete.
- Not Started: no production implementation exists yet.

Dependency rule:
- Do not start higher-level features that depend on incomplete durability, metadata, delete, or index semantics unless the work is explicitly marked as a prototype.
- Prototype functionality must not be treated as production complete.

Current production readiness summary:
- Phases 1-8 are not production complete yet.
- The current query pipeline is a functional foundation, not a production-grade graph database.
- The highest-priority blocker is transactional metadata and property-key handling because labels/properties are used by almost every query feature.

---

## Phase 1: Storage APIs Needed By Queries

Production Status: Foundation Implemented, not Production Complete.

Implemented foundation:
- Node/edge scans.
- Safe edge deletes.
- Safe node deletes.
- Transaction delete variants.

Production blockers:
- Label registration is not rollback-integrated with query transactions.
- Property-key dictionary does not exist yet.
- Property key collision strategy is not defined.
- Property name introspection is not available.

Dependencies:
- Blocks production completion of phases 4, 5, 6, and 8 because query execution, indexes, constraints, and result introspection depend on durable metadata semantics.

Goal: expose correct graph primitives so the executor does not reach into page internals.

### Add Scans

Files:
- `core/db/hive_db.rs`
- `core/storage/pager.rs`
- `core/storage/page/layout.rs`

Tasks:
- Add `HiveDb::scan_nodes() -> Result<Vec<(NodeId, NodeRecord)>, DbError>`.
- Add `HiveDb::scan_edges() -> Result<Vec<(EdgeId, EdgeRecord)>, DbError>`.
- Iterate all pages using `pager.page_count()`.
- Skip meta, label, overflow, empty, and unknown pages.
- Read `PageType::DataNode` and `PageType::DataEdge` pages only.
- Iterate live slots from `PageHeader.slot_count`.
- Deserialize records and return packed IDs with `pack_record_id(page_id, slot_id)`.
- Ignore deleted slots.

Definition of done:
- Tests create multiple pages worth of nodes/edges and scan all live records.
- Deleted slots are not returned once delete support exists.

### Add Safe Deletes

Files:
- `core/db/hive_db.rs`
- `core/transaction.rs`
- `core/storage/page/layout.rs`

Tasks:
- Add `HiveDb::delete_edge(edge_id)` with WAL-backed autocommit.
- Add `HiveDb::delete_node(node_id)` with WAL-backed autocommit.
- Add transaction variants: `tx.delete_edge`, `tx.delete_node`.
- `delete_node` must reject nodes that still have incident edges.
- Add `HiveDb::node_has_edges(node_id)` using `scan_edges()` until adjacency lists are maintained.
- Decrement durable metadata counts only if count semantics remain "live count".
- Decide and document whether meta counts mean total allocated records or live records.

Definition of done:
- Deleting an edge removes it from scans and point reads.
- Deleting an isolated node removes it from scans and point reads.
- Deleting a node with relationships returns a query/storage error.
- Rollback restores deleted records.
- WAL recovery preserves committed deletes and discards uncommitted deletes.

### Make Label And Property-Key Metadata Transactional

Files:
- `core/storage/label_store.rs`
- `core/db/hive_db.rs`
- future `core/storage/property_key_store.rs`

Tasks:
- Ensure label registration participates in before-image capture and WAL commit.
- Add a property-key dictionary instead of relying only on key hashes.
- Store property key IDs or keep hashes with collision strategy.
- Support property name introspection for `RETURN n` and debugging.

Definition of done:
- Labels created by queries survive crash recovery only when committed.
- Failed transactions do not leak labels or property keys.

---

## Phase 2: Query AST And Parser Refactor

Production Status: Foundation Implemented, not Production Complete.

Implemented foundation:
- Clause pipeline AST.
- Multi-clause parsing.
- Parser support for the current query subset.

Production blockers:
- Parser tests do not yet cover every supported syntax form.
- Unsupported Cypher syntax needs a stricter fail-fast matrix.
- Parameters, `WITH`, aggregation, `OPTIONAL MATCH`, multiple labels, and full `MERGE` syntax are not implemented.

Dependencies:
- Depends on phase 8 language decisions for final production grammar.
- Blocks production completion of phases 3 and 4 for syntax that is not yet represented in the AST.

Goal: support real multi-clause queries instead of isolated top-level statements.

Current limitation:
- `Statement::Set` and `Statement::Delete` are standalone, so variables are unbound.
- Production queries need `MATCH ... SET`, `MATCH ... DELETE`, `MERGE ... SET`, and `MATCH ... RETURN` pipelines.

Files:
- `parser/src/ast.rs`
- `parser/src/parser.rs`
- `parser/src/token.rs`
- `parser/src/lexer.rs`

### Change AST Shape

Replace top-level statement enum with clause pipelines:

```rust
pub struct Statement {
    pub clauses: Vec<Clause>,
}

pub enum Clause {
    Create(Pattern),
    Match(MatchClause),
    Where(Expression),
    Set(SetClause),
    Delete(DeleteClause),
    Merge(Pattern),
    Return(ReturnClause),
}
```

Tasks:
- Keep existing pattern, expression, return item, and relationship structs where possible.
- Add `DeleteClause` with variables and optional detach mode later.
- Decide whether `WHERE` is its own clause or remains attached to `MATCH`; production pipelines are easier with a `Filter` plan step either way.

### Parser Coverage

Tasks:
- Parse clause sequences until EOF.
- Support `MATCH ... WHERE ... SET ... RETURN ...`.
- Support `MATCH ... DELETE ...`.
- Support `CREATE ... RETURN ...`.
- Support `MERGE ... SET ... RETURN ...`.
- Support comma-separated delete variables: `DELETE n, r`.
- Add `DETACH DELETE` later after storage semantics are ready.
- Add `ON CREATE SET` and `ON MATCH SET` later for full `MERGE` semantics.

Definition of done:
- Parser tests cover valid multi-clause queries.
- Parser rejects unstructured or ambiguous clauses with useful errors.

---

## Phase 3: Planner Pipeline

Production Status: Foundation Implemented, not Production Complete.

Implemented foundation:
- Pipeline planning for the current query subset.
- Variable-scope validation.
- Clear rejection for unsupported variable-length traversal.

Production blockers:
- Planner tests do not yet cover every query family.
- Index-cost planning is not implemented.
- Relationship `MERGE`, aggregation, `WITH`, and optional-match planning are missing.

Dependencies:
- Depends on phase 2 for complete AST coverage.
- Depends on phase 6 for production index planning and uniqueness-aware `MERGE`.
- Blocks production completion of phase 4 for unsupported plan shapes.

Goal: convert clause AST into executable plan steps over row streams.

Files:
- `core/query/planner.rs`
- `core/query/utils.rs`

### Plan Model

Replace or extend current `QueryPlan` with pipeline steps:

```rust
pub enum PlanStep {
    CreateNode { variable: Option<String>, node: NodePattern },
    CreateRelationship { src: NodePattern, rel: RelationshipPattern, dst: NodePattern },
    MergeNode { variable: Option<String>, node: NodePattern },
    MergeRelationship { pattern: PathPattern },
    ScanNodes { variable: String, label: Option<String>, filter: Option<Expression>, index_hint: NodeIndexHint },
    TraverseEdges { from_var: String, edge_var: Option<String>, edge_type: Option<String>, direction: Direction, to_var: String, to_label: Option<String>, hops: Option<RelationshipLength> },
    Filter { condition: Expression },
    SetProperty { variable: String, key: String, value: Expression },
    Delete { variables: Vec<String>, detach: bool },
    Return { items: Vec<ReturnItem> },
}

pub struct QueryPlan {
    pub steps: Vec<PlanStep>,
}
```

Tasks:
- Preserve existing scan and traversal planning where possible.
- Move literal conversion for `SET` from planner to executor so expressions can use variables later.
- Track variable scope and reject unknown variables at planning time.
- Validate that `SET` and `DELETE` operate on bound node/edge variables.
- Reject unsupported variable-length traversals until implemented.

Definition of done:
- Planner tests verify produced steps for every query family.
- Invalid variable references fail before execution.

---

## Phase 4: Query Executor Core

Production Status: Foundation Implemented, not Production Complete.

Implemented foundation:
- Row bindings.
- Expression evaluation for the current value model.
- `CREATE`, `MATCH`, `WHERE`, `RETURN`, `SET`, `DELETE`, `DETACH DELETE`, and single-node `MERGE` for the current subset.

Production blockers:
- Label/property metadata is not fully transactional.
- Relationship `MERGE` is not implemented.
- Unique constraints do not exist, so `MERGE` is not production-safe under duplicate-risk workloads.
- Aggregation, `WITH`, `OPTIONAL MATCH`, and variable-length traversal execution are missing.
- Crash-style query recovery tests are incomplete.

Dependencies:
- Depends on phase 1 for transactional metadata and safe storage primitives.
- Depends on phase 3 for complete plan shapes.
- Depends on phase 6 for production-safe indexed/unique `MERGE`.
- Depends on phase 7 for production traversal performance and adjacency correctness.

Goal: execute all planned query steps using a row/binding pipeline.

Files:
- `core/query/executor.rs`
- `core/query/result.rs`
- `core/db/hive_db.rs`
- `core/transaction.rs`

### Runtime Row Model

Add internal executor types:

```rust
enum EntityRef {
    Node(NodeId),
    Edge(EdgeId),
    Value(Value),
}

type Row = HashMap<String, EntityRef>;
```

Tasks:
- Start each query with one empty row.
- Each plan step transforms `Vec<Row>` into `Vec<Row>` or a `QueryResult`.
- Keep executor internals private unless public APIs need them later.

### Expression Evaluation

Tasks:
- Evaluate literals.
- Evaluate variables.
- Evaluate node and edge properties.
- Evaluate `=`, `<>`, `>`, `>=`, `<`, `<=`.
- Evaluate `AND`, `OR`, and `NOT`.
- Define type comparison rules.
- Missing property should evaluate to `Value::Null` for query expressions, not low-level `ReadError`.
- Add truthiness rules for booleans and nulls.

### CREATE Execution

Tasks:
- Create nodes with labels and properties.
- Create relationships with labels and properties.
- Bind created variables into rows if variables are present.
- Use a single transaction for the whole query.
- Ensure label registration and property writes are part of the same commit unit.

### MATCH Execution

Tasks:
- Full-scan node matching.
- Label filtering.
- Property filtering.
- One-hop outgoing, incoming, and undirected traversal.
- Edge type filtering.
- Bind edge variables when present.
- Bind destination node variables.
- Avoid rebinding conflicts incorrectly: if a variable is already bound, new matches must equal the existing entity.

### WHERE Execution

Tasks:
- Filter row streams with expression evaluation.
- Support property predicates and boolean combinations.

### RETURN Execution

Tasks:
- Project expressions into columns.
- Use aliases when provided.
- Derive column names for simple expressions like `n.name`.
- Return node/edge IDs or map-like values for whole entity variables.
- Add stable result ordering later when `ORDER BY` exists.

### SET Execution

Tasks:
- Apply to every row in the current stream.
- Support node properties.
- Support edge properties.
- Evaluate right-hand side expressions per row.
- Run inside query transaction.

### DELETE Execution

Tasks:
- Delete edge variables.
- Delete node variables only if no incident edges exist.
- Deduplicate repeated variables/entities before deleting.
- Delete edges before nodes when both are present.
- Reject deleting values or unknown variables.
- Add `DETACH DELETE` later to delete incident edges automatically.

### MERGE Execution

Tasks:
- Implement node `MERGE` first using label and property full scan.
- If matching row exists, bind it.
- If no match exists, create node and set all literal properties.
- Relationship `MERGE` later: match existing relationship pattern, otherwise create missing relationship and endpoints according to Cypher semantics.
- Add `ON CREATE SET` and `ON MATCH SET` later.
- Make production-safe with unique indexes/constraints in the index phase.

Definition of done:
- End-to-end executor tests cover `CREATE`, `MATCH`, `WHERE`, `RETURN`, `SET`, `DELETE`, and `MERGE`.
- Mutating query failures rollback all earlier mutations in the same query.
- Crash-style tests verify committed query mutations recover.

---

## Phase 5: Transaction Semantics For Queries

Production Status: Foundation Implemented, not Production Complete.

Implemented foundation:
- `HiveDb::execute(query)` parses, plans, executes, commits on success, and rolls back on error.

Production blockers:
- Read-only queries still go through the same transaction path instead of avoiding WAL work where possible.
- Label/property metadata is not fully rollback-safe.
- Query-level WAL recovery tests are incomplete.

Dependencies:
- Depends on phase 1 for transactional metadata.
- Depends on phase 4 for executor rollback correctness.
- Blocks production completion of phases 6 and 8 because indexes and expanded language features must preserve query atomicity.

Goal: every query is an atomic unit of work.

Files:
- `core/query/executor.rs`
- `core/db/hive_db.rs`
- `core/transaction.rs`

Tasks:
- Add `HiveDb::execute(query: &str) -> Result<QueryResult, DbError>`.
- Parse, plan, and execute in one public API.
- Start a transaction for any query that may mutate data.
- Rollback on any execution error.
- Commit after successful execution.
- Keep read-only queries out of WAL when possible.
- Add explicit transaction query execution later if needed.

Definition of done:
- `CREATE ... SET ... RETURN` commits once.
- Failure in a later `SET` rolls back earlier `CREATE`.
- Query-level WAL entries recover exactly once.

---

## Phase 6: Indexes And Constraints

Production Status: Not Started.

Implemented foundation:
- Planner carries index hints for future integration.

Production blockers:
- Durable B-tree storage does not exist.
- Index insert/update/delete and rollback maintenance do not exist.
- Unique constraints do not exist.

Dependencies:
- Depends on phase 1 transactional metadata and property-key dictionary.
- Depends on phase 5 query atomicity.
- Blocks production-safe `MERGE` in phase 4 and many phase 8 features that need efficient lookup.

Goal: replace full scans with durable lookup structures without changing query behavior.

Files:
- `core/db/index.rs`
- new `core/storage/btree/*`
- `core/query/planner.rs`
- `core/query/executor.rs`

### B-Tree Storage

Tasks:
- Define index page types: interior and leaf.
- Store sorted keys and record ID payloads.
- Implement exact-match lookup.
- Implement insert and delete with WAL-backed page changes.
- Implement split and root growth.
- Implement range scan later.

### Index Types

Tasks:
- Node label index: label ID -> node IDs.
- Edge type index: label/type ID -> edge IDs.
- Node property index: `(label_id?, key_id/hash, value)` -> node IDs.
- Edge property index: `(type_id?, key_id/hash, value)` -> edge IDs.
- Unique node constraints for production-safe `MERGE`.

### Planner Integration

Tasks:
- Use `NodeIndexHint` to choose label/property indexes.
- Add edge index hints for relationship type scans.
- Keep full scan fallback.
- Add basic cost estimates later.

Definition of done:
- Indexed and full-scan plans return identical results.
- Indexes survive reopen and WAL recovery.
- Inserts, updates, deletes, and rollback keep indexes consistent.

---

## Phase 7: Graph Storage Correctness Improvements

Production Status: Foundation Implemented, not Production Complete.

Implemented foundation:
- Deletes use slotted-page dead slots.
- Safe node delete rejects incident relationships.
- `DETACH DELETE` can remove incident edges first through the executor.

Production blockers:
- Adjacency chains are not maintained.
- Persistent freelist is not implemented.
- Record-level reuse policy is incomplete.
- Page compaction/vacuum policy is incomplete.
- Deleted record flags are not consistently enforced as a storage-level invariant.

Dependencies:
- Depends on phase 1 safe delete primitives.
- Depends on phase 5 rollback semantics.
- Blocks production traversal performance in phase 4 and production index consistency in phase 6.

Goal: improve graph integrity, traversal speed, and space reuse.

Files:
- `core/storage/page/record.rs`
- `core/db/hive_db.rs`
- `core/storage/page/layout.rs`
- `core/storage/pager.rs`

Tasks:
- Maintain `first_out_edge`, `first_in_edge`, `next_out_edge`, and `next_in_edge` adjacency chains.
- Update adjacency chains on edge create and delete.
- Add freelist persistence instead of session-only page reuse.
- Add record-level reuse where possible.
- Add page compaction policy.
- Add vacuum or background reclamation later.
- Define and enforce deleted record flags consistently.

Definition of done:
- Traversals can use adjacency chains instead of scanning all edges.
- Freelist survives restart.
- Deletes reclaim reusable space safely.

---

## Phase 8: Query Language Expansion

Production Status: Foundation Implemented for selected syntax, not Production Complete.

Implemented foundation:
- `ORDER BY`.
- `SKIP`.
- `LIMIT`.
- `DETACH DELETE`.

Production blockers:
- `COUNT`, aggregation, and grouping are missing.
- `WITH` is missing.
- `OPTIONAL MATCH` is missing.
- `REMOVE` labels/properties is missing.
- Multiple labels per node are missing.
- Parameters are missing.
- Lists/maps are missing from the value model.
- Variable-length traversal execution is missing.

Dependencies:
- Depends on phase 2 for final AST grammar.
- Depends on phase 3 for planning new clause families.
- Depends on phase 4 for execution semantics.
- Depends on phase 6 for production performance of expanded query shapes.
- Depends on phase 7 for variable-length traversal performance and correctness.

Goal: support a useful production subset beyond the current parser.

Tasks:
- `ORDER BY`.
- `LIMIT` and `SKIP`.
- `COUNT`, basic aggregation, and grouping.
- `WITH` for multi-stage pipelines.
- `OPTIONAL MATCH`.
- `DETACH DELETE`.
- `REMOVE` labels/properties.
- Multiple labels per node.
- Relationship property predicates in pattern literals.
- Variable-length traversal execution.
- Parameters, e.g. `$name`.
- Query result maps/lists if the value model expands.

Definition of done:
- Parser, planner, executor, and tests exist for every supported clause.
- Unsupported Cypher syntax fails clearly instead of silently misbehaving.

---

## Phase 9: Concurrency And Isolation

Goal: allow safe concurrent access.

Tasks:
- Add coarse `Arc<RwLock<HiveDb>>` wrapper first.
- Define read/write transaction behavior.
- Add page-level locks later.
- Add lock ordering to prevent deadlocks.
- Add MVCC snapshots for read consistency.
- Add background checkpointing.
- Add async I/O only after sync correctness is stable.

Definition of done:
- Concurrent readers are safe.
- Writer exclusivity is clear.
- Crash recovery and checkpointing remain correct under concurrency.

---

## Phase 10: Observability And Operations

Goal: make the database debuggable and maintainable.

Tasks:
- Add page inspection tools.
- Add WAL inspection tools.
- Add query `EXPLAIN` output.
- Add query timing and row count metrics.
- Add storage integrity checker.
- Add index consistency checker.
- Add database statistics: page counts, dirty pages, cache hit rate, index sizes.

Definition of done:
- A corrupt or inconsistent database can be diagnosed with built-in tools.
- Query plans can be inspected before execution.

---

## Phase 11: Testing Strategy

Goal: prevent correctness regressions while the engine grows.

Test crates:
- `testing/rust`

Commands:

```bash
cargo fmt --check -p hive_core_testing
cargo check -p hive_core_testing --all-targets
cargo test -p hive_core_testing
```

Required test categories:

- Parser tests for every supported syntax form.
- Planner tests for every query shape.
- Executor tests for every clause and clause combination.
- Storage scan/delete tests.
- Transaction rollback tests for multi-step queries.
- WAL crash/recovery tests for query-created data.
- Index insert/update/delete/recovery tests.
- Property type tests for integer, float, boolean, short string, long string, and null.
- Graph integrity tests for node delete, edge delete, and detach delete.
- Fuzz-style parser tests later.
- Randomized operation tests comparing full scans vs indexes.

Minimum end-to-end examples:

```cypher
CREATE (a:Person {name: "Alice", age: 30}) RETURN a.name
MATCH (a:Person {name: "Alice"}) RETURN a.age
MATCH (a:Person) WHERE a.age >= 30 SET a.active = true RETURN a.active
CREATE (a:Person {name: "Alice"})-[:KNOWS {since: 2020}]->(b:Person {name: "Bob"})
MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN a.name, r.since, b.name
MERGE (p:Person {email: "alice@example.com"}) RETURN p.email
MATCH (a:Person)-[r:KNOWS]->(b:Person) DELETE r
MATCH (a:Person {name: "Alice"}) DELETE a
```

---

## Dependency-Prioritized Production Order

This order replaces prototype-first sequencing. Do not mark a phase production complete until every dependency and definition-of-done item is satisfied.

1. Finish phase 1 transactional metadata.
Required before: production query execution, indexes, constraints, introspection, and rollback-safe labels/properties.
Work: make label registration participate in before-image capture and WAL commit, add property-key dictionary, define key collision strategy, add property name introspection, add rollback/recovery tests.

2. Harden phase 5 query transaction semantics.
Required before: production executor, indexes, expanded query language.
Work: ensure mutating queries commit exactly once, rollback every page touched by query execution, avoid WAL for read-only queries where possible, add crash/recovery tests for committed and failed queries.

3. Finish phase 1 scan/delete production tests.
Required before: production delete semantics and graph integrity.
Work: add multi-page scan tests, delete scan tests, point-read-after-delete tests, rollback delete tests, WAL recovery delete tests.

4. Finish phase 2 parser production coverage.
Required before: planner/executor can be considered complete for any syntax.
Work: parser tests for every supported clause, strict rejection tests for unsupported Cypher syntax, final grammar decisions for `WHERE`, `DELETE`, `DETACH DELETE`, `MERGE`, and return modifiers.

5. Finish phase 3 planner production coverage.
Required before: executor production completion.
Work: planner tests for every query family, invalid variable tests, complete plan shape validation, clear unsupported-feature errors.

6. Finish phase 4 executor production subset.
Required before: indexing and language expansion can be trusted.
Work: complete rollback tests for multi-step mutations, crash-style query tests, relationship `MERGE` semantics, stricter type comparison rules, whole-entity return representation, and complete executor test matrix.

7. Implement phase 7 adjacency chains before performance-sensitive traversal.
Required before: production traversal performance and variable-length traversal.
Work: maintain `first_out_edge`, `first_in_edge`, `next_out_edge`, and `next_in_edge` on edge create/delete; test rollback and recovery of adjacency updates.

8. Implement phase 7 persistent space reuse.
Required before: long-running production workloads.
Work: persistent freelist, record-level reuse policy, page compaction policy, deleted-record flag invariants, vacuum/background reclamation design.

9. Implement phase 6 durable B-tree storage.
Required before: production indexes and constraints.
Work: index page layout, exact lookup, insert/delete, split/root growth, WAL-backed page changes, recovery tests.

10. Implement phase 6 index maintenance.
Required before: indexed queries can be trusted.
Work: node label index, edge type index, node property index, edge property index, update/delete rollback consistency, full-scan vs indexed randomized comparisons.

11. Implement phase 6 uniqueness constraints.
Required before: production-safe `MERGE`.
Work: unique node constraints, deterministic conflict handling, constraint recovery tests, planner/executor integration.

12. Complete production-safe `MERGE`.
Required before: claiming Cypher-like mutation semantics are production grade.
Work: relationship `MERGE`, `ON CREATE SET`, `ON MATCH SET`, uniqueness-backed node merge, concurrency-safe duplicate prevention later with isolation work.

13. Expand phase 8 language only after storage/query foundations are production complete.
Required before: exposing broader Cypher support.
Work order: parameters, `WITH`, aggregation/`COUNT`, `OPTIONAL MATCH`, `REMOVE`, multiple labels, relationship property predicates, lists/maps, variable-length traversal.

14. Add phase 9 concurrency and isolation.
Required before: concurrent production deployments.
Work: coarse `Arc<RwLock<HiveDb>>`, read/write transaction rules, lock ordering, page-level locks later, MVCC snapshots, background checkpointing.

15. Add phase 10 observability and operations.
Required before: operational production readiness.
Work: page inspector, WAL inspector, `EXPLAIN`, query timings, storage integrity checker, index consistency checker, database statistics.

16. Complete phase 11 testing strategy continuously.
Required before: every production-complete phase signoff.
Work: parser, planner, executor, storage, WAL, recovery, index, graph integrity, fuzz-style, and randomized tests.

---

## Production Readiness Checklist

- Query executor supports `CREATE`, `MATCH`, `WHERE`, `RETURN`, `SET`, `DELETE`, and `MERGE`.
- Multi-clause queries are parsed, planned, and executed as row pipelines.
- Every mutating query is atomic and WAL recoverable.
- Deletes preserve graph integrity.
- Labels and property metadata are transactional.
- Indexes are durable and transactionally consistent.
- `MERGE` uses indexes/constraints where required to avoid duplicates.
- Page, record, and index corruption can be detected.
- Tests cover normal execution, rollback, reopen, and crash recovery.
- Examples run end-to-end through public APIs.
