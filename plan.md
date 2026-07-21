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
- Parser and planner for a subset of Cypher-like statements.
- Query executor is currently stubbed.

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

---

## Phase 1: Storage APIs Needed By Queries

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

## Suggested Implementation Order

1. Add `scan_nodes()` and `scan_edges()`.
2. Add executor row/binding model.
3. Implement `CREATE`, `MATCH`, `WHERE`, and `RETURN` with the current parser shape.
4. Refactor parser AST to clause pipelines.
5. Refactor planner to pipeline steps.
6. Add query-level transaction execution.
7. Implement `SET` after `MATCH`.
8. Add storage-level `delete_edge()` and safe `delete_node()`.
9. Implement `DELETE`.
10. Implement single-node `MERGE` with full scan.
11. Implement relationship `MERGE`.
12. Make labels/property-key metadata transactional.
13. Add B-tree indexes for labels, edge types, and properties.
14. Add unique constraints for production-safe `MERGE`.
15. Add adjacency chain maintenance and traversal optimization.
16. Add persistent freelist and compaction.
17. Add query language expansion: `ORDER BY`, `LIMIT`, `WITH`, aggregation, `OPTIONAL MATCH`, `DETACH DELETE`.
18. Add concurrency and MVCC.
19. Add observability and integrity tooling.

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
