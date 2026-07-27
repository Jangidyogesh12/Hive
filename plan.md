# Hive Production Implementation Steps

This file is the practical next-step guide after the current implementation. It is ordered so that no step depends on a later step. If a step is not complete, do not treat later dependent work as production-grade.

Current state:
- Parser, planner, executor, scans, safe deletes, `DETACH DELETE`, `ORDER BY`, `SKIP`, `LIMIT`, and single-node `MERGE` exist as a foundation.
- Production blockers remain around transactional metadata, query recovery tests, indexes, adjacency chains, and broader language support.

Rule:
- Implement steps in this order.
- Do not skip a step unless it is explicitly not needed for the production target.
- A step is complete only when code, tests, rollback behavior, and recovery behavior are done.

## [x] Step 1: Make Label Metadata Transactional

Why this is next:
- Labels are used by `CREATE`, `MATCH`, `MERGE`, indexes, constraints, and query results.
- If label registration is not rollback-safe, any mutating query can leak metadata after failure.

What to implement:
- Change label registration so it can capture before-images when called inside a transaction.
- Add `Transaction::register_label(...)` that records label page and meta page before-images.
- Ensure `CREATE (n:Person)` registers `Person` inside the same query transaction as node creation.
- Ensure rollback removes newly registered labels when the query fails.
- Ensure committed labels survive reopen and WAL recovery.

Files to study/change:
- `core/storage/label_store.rs`
- `core/db/hive_db.rs`
- `core/transaction.rs`
- `core/query/executor.rs`
- `testing/rust/core/db/label_test.rs`
- `testing/rust/core/query/mod.rs`

Database topics to know first:
- Transaction atomicity.
- Write-ahead logging basics.
- Before-image rollback.
- Metadata pages and dictionary tables.

DSA/storage topics to know first:
- Slotted pages.
- Page headers and slot tables.
- Linear scan over page slots.

Rust concepts to know first:
- `&mut` borrowing across function calls.
- `Option<&mut Vec<T>>` patterns.
- Error propagation with `?`.
- Ownership of `String` vs `&str`.

Definition of done:
- Failed query-created labels do not remain visible after rollback.
- Committed query-created labels survive reopen.
- Committed query-created labels survive WAL recovery.
- Existing label tests still pass.

## [x] Step 2: Add A Transactional Property-Key Dictionary

Why this comes after Step 1:
- It should follow the same metadata transaction pattern as labels.
- Property keys are needed before indexes, introspection, and safe property collision handling.

What to implement:
- Add `core/storage/property_key_store.rs`.
- Store entries like `key_id -> key_name` in dedicated `PropertyKeyData` pages.
- Add `HiveDb::register_property_key(name)` and `Transaction::register_property_key(name)`.
- Register property keys inside `set_node_property_inner` and `set_edge_property_inner` or at the query layer before writes.
- Ensure property-key writes participate in rollback and WAL commit.
- Add `get_property_key_name(key_id)` and `find_property_key(name)`.

Implemented notes:
- `PropertyKeyData` pages store only property-key dictionary metadata, for example `1 -> "name"`.
- Property values are still stored inline in `NodeRecord.properties` and `EdgeRecord.properties`; long strings still use overflow pages.
- There is no separate `DataProperty` page type in the current production plan because external property-value pages are not part of these ordered steps.
- Step 3 is still required before node/edge records directly reference property keys by `key_id`; Step 2 only creates and maintains the transactional dictionary.

Files to study/change:
- `core/storage/label_store.rs`
- `core/storage/page/layout.rs`
- `core/storage/page/format.rs`
- `core/storage/mod.rs`
- `core/db/hive_db.rs`
- `core/transaction.rs`
- `core/value.rs`

Database topics to know first:
- System catalogs/dictionaries.
- Schema metadata durability.
- Transactional metadata updates.

DSA/storage topics to know first:
- Append-only dictionary records.
- Fixed-size vs variable-size records.
- Lookup by scan before indexes exist.

Rust concepts to know first:
- Module creation and exports.
- Struct methods with `impl`.
- Byte encoding/decoding with slices.
- `Vec<u8>` record construction.

Definition of done:
- Property keys created by committed queries survive reopen.
- Property keys created by failed transactions are rolled back.
- Duplicate key registration returns the existing key id.
- Tests cover node and edge property-key registration.

## [x] Step 3: Replace Property Hash-Only Semantics With Collision-Safe Key Identity

Why this comes after Step 2:
- You need real property-key IDs before making property lookup collision-safe.

What to implement:
- Decide the on-record property entry strategy:
  - Preferred: store `key_id` in `PropertyEntry` and keep hash only as optional acceleration later.
  - Alternative: store both `key_hash` and `key_id`.
- Update node and edge property writes to store key identity safely.
- Update property reads to resolve by property-key dictionary identity, not only hash.
- Define migration policy for current files. If no compatibility is required, document that existing test DBs should be recreated.

Implemented notes:
- `PropertyEntry` now stores `key_id` instead of `key_hash`.
- Node and edge property writes register/resolve the property key and store the resulting `key_id` on the record.
- Node and edge property reads resolve property names through the property-key dictionary, then match entries by `key_id`.
- The unused hash helper and hash-only record semantics were removed.
- Existing database files using the old hash-based property entry layout should be recreated; no compatibility migration is implemented.

Files to study/change:
- `core/storage/page/record.rs`
- `core/db/hive_db.rs`
- `core/value.rs`
- `testing/rust/core/db/property_test.rs`

Database topics to know first:
- Key dictionaries.
- Hash collision handling.
- On-disk format versioning.

DSA/storage topics to know first:
- Record layout changes.
- Backward compatibility vs format reset.
- Equality by ID vs equality by hash.

Rust concepts to know first:
- Updating serialized formats safely.
- Pattern matching enum values.
- Test-driven refactors.

Definition of done:
- Property lookup is not dependent on hash uniqueness.
- Tests prove two distinct property names cannot incorrectly read the same value due to hash-only lookup.
- Existing property read/write tests pass.

## [x] Step 4: Add Property Name Introspection And Whole-Entity Return Support

Why this comes after Step 3:
- Introspection needs property-key names to reconstruct entity property maps.

What to implement:
- Add APIs to list properties on a node or edge with names and values.
- Update `RETURN n` and `RETURN r` to produce useful entity values or a stable string/map representation.
- Decide whether `Value` should grow `Map`/`List` variants now or whether whole-entity return should be a formatted string until the value model expands.

Files to study/change:
- `core/value.rs`
- `core/query/result.rs`
- `core/query/executor.rs`
- `core/db/hive_db.rs`

Database topics to know first:
- Result serialization.
- Entity projection semantics.
- Property introspection.

DSA/storage topics to know first:
- Iterating property entries.
- Joining key IDs to dictionary names.

Rust concepts to know first:
- Enum extension.
- Formatting with `Display`.
- Cloning vs borrowing result values.

Definition of done:
- `MATCH (n:Person) RETURN n` returns inspectable node data.
- Entity return includes ID, label/type, and named properties.
- Tests cover node and edge entity returns.

Implemented notes:
- Added `Value::Map(HashMap<String, Value>)` and `Value::List(Vec<Value>)` variants.
- Added `Display` implementation for all `Value` variants including Map and List.
- Added `HiveDb::list_node_properties(node_id)` and `HiveDb::list_edge_properties(edge_id)` that resolve `key_id` to key names via the property-key dictionary.
- Updated `Transaction` to expose `list_node_properties` and `list_edge_properties`.
- Updated executor so `RETURN n` produces a `Value::Map` with `id`, `label`, and `properties` fields.
- Updated executor so `RETURN r` produces a `Value::Map` with `id`, `type`, `src`, `dst`, and `properties` fields.

## [x] Step 5: Harden Query Transaction Semantics

Why this comes after metadata steps:
- Query rollback cannot be production-grade until all query-touched metadata is transactional.

What to implement:
- Ensure mutating queries open one transaction and commit exactly once.
- Ensure any execution error rolls back all pages touched by nodes, edges, labels, property keys, and overflow strings.
- Add read-only execution path that avoids WAL work where possible.
- Add tests for failure in later clauses rolling back earlier clauses.

Files to study/change:
- `core/query/executor.rs`
- `core/db/hive_db.rs`
- `core/transaction.rs`
- `core/wal/*`
- `testing/rust/core/db/wal_commit_test.rs`
- `testing/rust/core/query/mod.rs`

Database topics to know first:
- ACID atomicity and durability.
- WAL commit protocol.
- Crash recovery redo.
- Rollback using before-images.

DSA/storage topics to know first:
- Dirty page tracking.
- Page image recovery.

Rust concepts to know first:
- Drop semantics and explicit commit/rollback.
- Borrowing `&mut HiveDb` through `Transaction`.
- Error handling without losing rollback path.

Definition of done:
- `CREATE ... SET ... RETURN` commits once.
- Failure in a later clause rolls back earlier node/edge/metadata changes.
- Read-only queries do not append WAL entries.
- Crash/recovery tests prove committed query mutations recover exactly once.

Implementation notes:
- Added `QueryPlan::is_read_only()` that checks all steps are read-only (ScanNodes, TraverseEdges, Filter, Return).
- Added `HiveDb::commit_readonly()` that marks dirty pages clean without WAL work.
- Added `Transaction::commit_readonly()` that delegates to `commit_readonly()`.
- Updated `executor::execute()` to detect read-only plans and use the WAL-free commit path.
- Read-only queries (MATCH ... RETURN) produce zero WAL entries even when registering new labels.
- Mutating queries (CREATE, SET, DELETE) still go through full WAL commit for durability.
- Added 8 new tests covering: read-only no-WAL, read-only with new label, crash recovery for execute(), rollback on failure, and more.

## [x] Step 6: Complete Storage Scan/Delete Production Tests

Why this comes after transaction hardening:
- Delete behavior must be proven under rollback and recovery before adjacency chains and indexes depend on it.

What to implement:
- Multi-page node scan tests.
- Multi-page edge scan tests.
- Deleted slots skipped by scans.
- Point reads fail for deleted records.
- Rollback restores deleted records.
- WAL recovery preserves committed deletes and discards uncommitted deletes.

Files to study/change:
- `core/db/hive_db.rs`
- `core/storage/page/format.rs`
- `testing/rust/core/db/node_test.rs`
- `testing/rust/core/db/edge_test.rs`
- `testing/rust/core/db/wal_commit_test.rs`

Database topics to know first:
- Logical delete vs physical delete.
- Recovery testing.
- Record visibility.

DSA/storage topics to know first:
- Slot tables.
- Dead slots.
- Freeblocks.

Rust concepts to know first:
- Test helpers.
- Temporary directories.
- Assertions over `Result`.

Definition of done:
- Scan/delete tests cover normal, rollback, reopen, and recovery behavior.
- Meta count semantics are documented as allocation counters or live counters.

Implementation notes:
- Added 14 new tests across node_test.rs, edge_test.rs, and wal_commit_test.rs.
- Node tests: multi-page scan (200 nodes), scan skips deleted slots, point read fails for deleted record, rollback restores deleted node, delete persists after reopen.
- Edge tests: multi-page scan (200 edges), scan skips deleted slots, point read fails for deleted record, rollback restores deleted edge, delete persists after reopen.
- WAL tests: committed delete survives crash recovery, uncommitted delete discarded on recovery, committed edge delete survives crash recovery, delete after checkpoint survives reopen.
- Documented `MetaHeader.node_count` and `MetaHeader.edge_count` as allocation counters (never decremented on delete).
- Total test count: 186 (up from 164 at start of Step 5, 307 after Step 8).

## [x] Step 7: Add Parser Production Test Coverage

Why this comes after storage/query metadata foundations:
- The parser already supports a subset, but production requires confidence that accepted syntax is intentional and rejected syntax fails clearly.

What to implement:
- Parser tests for every currently supported clause and combination.
- Rejection tests for unsupported syntax.
- Tests for `ORDER BY`, `SKIP`, `LIMIT`, and `DETACH DELETE`.
- Tests for bad clause order and unknown tokens.

Files to study/change:
- `parser/src/ast.rs`
- `parser/src/lexer.rs`
- `parser/src/parser.rs`
- `parser/src/token.rs`
- `testing/rust/core/parser/` (9 test files)

Database topics to know first:
- Query language grammar.
- Clause pipeline semantics.

DSA/storage topics to know first:
- Recursive-descent parsing.
- Operator precedence parsing.

Rust concepts to know first:
- Lifetimes for borrowed input tokens.
- Pattern matching.
- Unit tests over enums/structs.

Definition of done:
- Every supported syntax form has parser tests.
- Unsupported syntax fails with useful errors.
- Parser tests can be run independently.

Implementation notes (2026-07-26):
- Added 79 tests across 9 files in `testing/rust/core/parser/`:
  - `create.rs` - CREATE nodes, paths, edges (outgoing/incoming/undirected), multi-hop, properties
  - `match_patterns.rs` - MATCH node/path patterns, all directions, variable-length relationships
  - `where_tests.rs` - WHERE with all comparison operators, AND/OR/NOT, precedence
  - `set_tests.rs` - SET with integer/string/boolean/float values, binary expressions
  - `delete_tests.rs` - DELETE / DETACH DELETE (single/multiple variables)
  - `merge_tests.rs` - MERGE nodes and paths
  - `return_tests.rs` - RETURN properties, aliases, literals, variables, ORDER BY, SKIP, LIMIT
  - `pipeline_tests.rs` - Multi-clause pipelines, semicolons, comments, case insensitivity
  - `error_tests.rs` - Empty input, unsupported keywords, bare expressions, trailing garbage, missing tokens, unterminated strings, invalid ranges
- Added `pub use hive_parser::error;` to `core/query/mod.rs` for error type access.
- One limitation discovered: parser does not support unary minus (`-5`) in expressions.

## [x] Step 8: Add Planner Production Test Coverage

Why this comes after parser tests:
- Planner tests depend on stable AST output.

What to implement:
- Tests for planned steps for every supported query family.
- Tests for invalid variable references.
- Tests for unsupported variable-length traversal rejection.
- Tests for index hints produced from label/property equality.

Files to study/change:
- `core/query/planner.rs`
- `core/query/utils.rs`
- `testing/rust/core/query/mod.rs`

Database topics to know first:
- Logical query plans.
- Predicate pushdown.
- Scope checking.

DSA/storage topics to know first:
- Graph pattern decomposition.
- Row pipeline planning.

Rust concepts to know first:
- Comparing enums in tests.
- `HashSet` scope tracking.
- `Option` and `Result` composition.

Definition of done:
- Planner tests cover create, match, traversal, where, set, delete, merge, return, order/limit.
- Invalid plans fail before executor runs.

Implementation notes (2026-07-27):
- Added 42 tests in `testing/rust/core/query/planner_test.rs`:
  - CREATE: node with label, properties, relationship path, relationship properties, multi-hop rejection, node without label/variable
  - MATCH: node with/without label, path traversal, incoming/undirected direction, variable requires variable
  - WHERE: filter step, unknown variable reference error
  - SET: property step, unknown variable reference error
  - DELETE: single/multiple variables, DETACH DELETE, unknown variable reference error
  - MERGE: node, path rejection
  - RETURN: variable, property, ORDER BY (ASC/DESC), SKIP/LIMIT, multiple items, unknown variable reference error
  - INDEX HINTS: label-only, label+property, property-only, full scan, equality vs non-equality hint distinction
  - VARIABLE-LENGTH TRAVERSAL: bounded and unbounded rejection
  - COMPLEX PIPELINES: multi-clause combinations, is_read_only detection
- All 42 tests pass. Formatting, clippy, and check pass.

## [x] Step 9: Complete Executor Production Subset Tests And Semantics

Why this comes after planner tests:
- Executor correctness depends on stable planned steps.

What to implement:
- Full executor matrix for supported clauses and combinations.
- Stricter type comparison rules.
- Rebinding conflict tests.
- Missing-property null behavior tests.
- Whole-entity return tests from Step 4.
- Relationship `MERGE` can be deferred until indexes/constraints if production-safe semantics require uniqueness.

Files to study/change:
- `core/query/executor.rs`
- `core/query/result.rs`
- `testing/rust/core/query/mod.rs`

Database topics to know first:
- Volcano/iterator execution model.
- Row binding model.
- Expression evaluation.
- Null semantics.

DSA/storage topics to know first:
- HashMap row environments.
- Deduplication with HashSet.
- Graph traversal basics.

Rust concepts to know first:
- Borrow checker with mutable transactions.
- Cloning small row maps safely.
- Enum-based runtime values.

Definition of done:
- Supported query subset is fully tested end to end.
- Mutating query failures rollback all changes.
- Query result shape is stable and documented.

Implementation notes (2026-07-27):
- Added 69 tests in `testing/rust/core/query/executor_test.rs`:
  - CREATE: node with/without label/variable, multiple nodes, relationship with properties, variable reuse in single statement
  - MATCH: by label, all nodes, path traversal (outgoing/undirected/incoming), empty results
  - WHERE: equality, inequality operators (>, >=, <, <=, <>), AND/OR logic, NOT operator, string comparison, traversal filter
  - SET: node/edge property, add new property, overwrite, literal values, sequential multi-property, type change
  - DELETE: node, edge, DETACH DELETE, multiple variables, delete all matched
  - MERGE: create new, reuse existing, different properties separate, without label, after create
  - RETURN: multiple columns, aliases, literals, empty result, whole-entity node/edge map (Value::Map)
  - ORDER BY: ASC, DESC, string ordering
  - SKIP/LIMIT: skip, limit, both, skip exceeding count
  - NULL BEHAVIOR: missing property returns null, null in comparison, null sorts before values
  - ROLLBACK: on SET/DELETE unknown variable, preserves entities on failed DETACH
  - REBINDING: scan rebinds to each match, traversal rebinds to each edge
  - TYPE COMPARISON: integer/float cross-type, boolean equality, string ordering
  - COMPLEX PIPELINES: CREATE+MATCH+SET+RETURN, MATCH+TRAVERSE+SET+RETURN, MATCH+WHERE+DELETE, MERGE+SET+RETURN
- All 69 tests pass. Total workspace tests: 376 (up from 307 before Step 9).

## [x] Step 10: Maintain Adjacency Chains On Edge Create/Delete

Why this comes after delete and transaction tests:
- Adjacency chains are extra persistent pointers and must rollback/recover correctly.

What to implement:
- On edge create, update source node `first_out_edge` and destination node `first_in_edge`.
- Set edge `next_out_edge` and `next_in_edge` to preserve linked lists.
- On edge delete, unlink the edge from both chains.
- Update traversal to use adjacency chains instead of scanning all edges when possible.

Files to study/change:
- `core/storage/page/record.rs`
- `core/db/hive_db.rs`
- `core/query/executor.rs`
- `testing/rust/core/db/edge_test.rs`
- `testing/rust/core/query/mod.rs`

Database topics to know first:
- Graph adjacency lists.
- Persistent pointer maintenance.
- Rollback of multi-record updates.

DSA/storage topics to know first:
- Singly linked lists.
- Linked-list deletion.
- Graph traversal by adjacency.

Rust concepts to know first:
- Updating multiple records in one transaction.
- Avoiding stale IDs.
- Helper functions for repeated page-record updates.

Definition of done:
- Traversal can use adjacency chains.
- Edge create/delete updates chains correctly.
- Rollback and recovery preserve chain correctness.

## [ ] Step 11: Add Persistent Freelist And Record Reuse Policy

Why this comes after adjacency chains:
- Space reuse must not break graph pointers or recovery.

What to implement:
- Persist database-level free pages instead of session-only free pages.
- Define record-level reuse policy for dead slots.
- Define page compaction policy.
- Add storage integrity tests for reuse after reopen.

Files to study/change:
- `core/storage/pager.rs`
- `core/storage/page/layout.rs`
- `core/storage/page/format.rs`
- `core/db/hive_db.rs`

Database topics to know first:
- Free page lists.
- Space reclamation.
- Vacuum/compaction tradeoffs.

DSA/storage topics to know first:
- Linked freelists.
- Fragmentation.
- Slotted-page compaction.

Rust concepts to know first:
- Byte-level page mutation.
- Persistent state encoded in structs.
- Careful test isolation.

Definition of done:
- Freed pages survive restart.
- Deleted record space is safely reusable.
- Compaction does not invalidate live record IDs unless explicitly designed.

## [ ] Step 12: Implement Durable B-Tree Page Storage

Why this comes after metadata and storage correctness:
- Index keys need stable property-key metadata and recovery-safe page mutation.

What to implement:
- Add B-tree page layout for interior and leaf pages.
- Store sorted keys and record ID payloads.
- Implement exact lookup.
- Implement insert/delete.
- Implement split and root growth.
- WAL-protect all index page changes.

Files to study/change:
- `core/db/index.rs`
- New `core/storage/btree/*`
- `core/storage/page/format.rs`
- `core/storage/pager.rs`
- `core/wal/*`

Database topics to know first:
- B-tree/B+tree indexes.
- Page splits.
- Root growth.
- Search key encoding.
- WAL for index pages.

DSA/storage topics to know first:
- Binary search.
- Sorted arrays.
- Tree insertion and deletion.
- Page-oriented B+trees.

Rust concepts to know first:
- Slices and binary search.
- Generic key encoding or enum key types.
- Ownership when splitting vectors/pages.

Definition of done:
- Exact-match B-tree lookup works after reopen.
- Inserts and deletes survive WAL recovery.
- Split/root growth tests pass.

## [ ] Step 13: Add Index Types And Maintenance

Why this comes after B-tree storage:
- Index types need a durable lookup structure first.

What to implement:
- Node label index.
- Edge type index.
- Node property index.
- Edge property index.
- Maintain indexes on create, set/update, delete, rollback, and recovery.
- Keep full scan fallback for correctness comparison.

Files to study/change:
- `core/db/index.rs`
- `core/db/hive_db.rs`
- `core/transaction.rs`
- `core/query/planner.rs`
- `core/query/executor.rs`

Database topics to know first:
- Secondary indexes.
- Covering vs non-covering indexes.
- Index consistency.
- Write amplification.

DSA/storage topics to know first:
- Composite keys.
- Duplicate values in indexes.
- Set/list payloads for one key to many record IDs.

Rust concepts to know first:
- Key serialization.
- Ordering implementations.
- Integration tests with randomized operations.

Definition of done:
- Indexed plans and full-scan plans return identical results.
- Index maintenance is rollback-safe.
- Indexes survive reopen and recovery.

## [ ] Step 14: Add Unique Constraints

Why this comes after index maintenance:
- Unique constraints are special indexes with conflict rules.

What to implement:
- Unique node constraints by label and property key.
- Constraint creation and metadata persistence.
- Constraint enforcement on create, set, merge, rollback, and recovery.
- Error handling for uniqueness conflicts.

Files to study/change:
- `core/db/index.rs`
- `core/db/hive_db.rs`
- `core/query/executor.rs`
- New schema/constraint metadata storage if needed.

Database topics to know first:
- Unique indexes.
- Constraint enforcement.
- Conflict handling.
- Schema metadata.

DSA/storage topics to know first:
- One-to-one index keys.
- Duplicate detection.

Rust concepts to know first:
- Custom error variants.
- Atomic multi-structure updates.
- Test fixtures for conflicts.

Definition of done:
- Duplicate constrained property insert fails.
- Rollback does not leave stale uniqueness entries.
- Recovery preserves committed constraints.

## [ ] Step 15: Complete Production-Safe MERGE

Why this comes after unique constraints:
- Production `MERGE` must not create duplicates under deterministic uniqueness rules.

What to implement:
- Unique-index-backed node `MERGE`.
- Relationship `MERGE` semantics.
- `ON CREATE SET`.
- `ON MATCH SET`.
- Tests for duplicate prevention.

Files to study/change:
- `parser/src/*`
- `core/query/planner.rs`
- `core/query/executor.rs`
- `core/db/index.rs`

Database topics to know first:
- Upsert semantics.
- Idempotent writes.
- Constraint-backed merge.

DSA/storage topics to know first:
- Indexed lookup before insert.
- Composite relationship identity.

Rust concepts to know first:
- Parser AST extension.
- Planner/executor coordination.
- Enum evolution without breaking tests.

Definition of done:
- Repeated `MERGE` does not create duplicates.
- `ON CREATE SET` and `ON MATCH SET` work.
- Relationship `MERGE` is deterministic and tested.

## [ ] Step 16: Add Query Parameters

Why this comes before broader language expansion:
- Parameters are needed for embedded DB APIs and safer application queries.

What to implement:
- Parse `$name` parameters.
- Add parameter map to `HiveDb::execute_with_params`.
- Bind parameters during expression evaluation.
- Reject missing parameters clearly.

Files to study/change:
- `parser/src/token.rs`
- `parser/src/lexer.rs`
- `parser/src/ast.rs`
- `parser/src/parser.rs`
- `core/query/executor.rs`
- `core/db/hive_db.rs`

Database topics to know first:
- Prepared statement parameters.
- Query binding.

DSA/storage topics to know first:
- HashMap lookup.

Rust concepts to know first:
- API design with borrowed vs owned parameter maps.
- Enum variants for parameter expressions.
- Lifetimes if borrowing parameter keys/values.

Definition of done:
- `$name` works in `CREATE`, `MATCH`, `WHERE`, `SET`, and `MERGE`.
- Missing parameter returns a query error.

## [ ] Step 17: Add WITH Pipeline Clause

Why this comes after parameters and stable executor pipeline:
- `WITH` changes variable scope and row projection between query stages.

What to implement:
- Parse `WITH` return-like projection.
- Planner should create a projection step that replaces row scope.
- Executor should project rows and continue with new bindings.

Files to study/change:
- `parser/src/*`
- `core/query/planner.rs`
- `core/query/executor.rs`

Database topics to know first:
- Query pipelines.
- Scope boundaries.
- Projection.

DSA/storage topics to know first:
- Row transformation.
- HashMap remapping.

Rust concepts to know first:
- Moving values between maps.
- Reusing return projection logic.

Definition of done:
- `MATCH ... WITH ... MATCH ... RETURN ...` works.
- Variables not projected by `WITH` are not visible later.

## [ ] Step 18: Add Aggregation And COUNT

Why this comes after WITH:
- Aggregation needs clear grouping scope and projection rules.

What to implement:
- Parse `COUNT(...)`.
- Add aggregate expressions.
- Group rows by non-aggregate return expressions.
- Implement count aggregation.

Files to study/change:
- `parser/src/ast.rs`
- `parser/src/parser.rs`
- `core/query/planner.rs`
- `core/query/executor.rs`
- `core/query/result.rs`

Database topics to know first:
- Aggregation.
- Group-by semantics.
- Null handling in aggregates.

DSA/storage topics to know first:
- HashMap grouping.
- Accumulators.

Rust concepts to know first:
- Hashing custom keys.
- Accumulator structs/enums.
- Sorting/grouping result rows.

Definition of done:
- `MATCH (n) RETURN COUNT(n)` works.
- Grouped aggregation works for simple keys.

## [ ] Step 19: Add OPTIONAL MATCH

Why this comes after aggregation/WITH foundation:
- Optional matching needs row-preserving null-extension behavior.

What to implement:
- Parse `OPTIONAL MATCH`.
- Planner emits optional scan/traversal steps.
- Executor preserves input rows when no match exists and binds missing variables to null-like values.

Files to study/change:
- `parser/src/*`
- `core/query/planner.rs`
- `core/query/executor.rs`

Database topics to know first:
- Outer join semantics.
- Null propagation.

DSA/storage topics to know first:
- Row expansion vs row preservation.

Rust concepts to know first:
- Optional bindings.
- Enum variants for null/missing entities.

Definition of done:
- Optional matches preserve rows without matches.
- Returned missing properties are null.

## [ ] Step 20: Add REMOVE And Multiple Labels

Why this comes after metadata and indexes:
- Label/property removal must update dictionaries, records, and indexes consistently.

What to implement:
- Multiple labels per node record or label-list storage.
- `REMOVE n:Label`.
- `REMOVE n.property`.
- Index maintenance for label/property removal.

Files to study/change:
- `core/storage/page/record.rs`
- `core/db/hive_db.rs`
- `core/db/index.rs`
- `parser/src/*`
- `core/query/*`

Database topics to know first:
- Multi-valued labels.
- Schema/index maintenance on delete/update.

DSA/storage topics to know first:
- Small vectors in records.
- Set membership.

Rust concepts to know first:
- Variable-length record updates.
- Vector retain/filter patterns.

Definition of done:
- Nodes can have multiple labels.
- Label/property removal is transactional and index-safe.

## [ ] Step 21: Add Variable-Length Traversal

Why this comes after adjacency chains:
- Without adjacency chains, variable-length traversal is too expensive and harder to bound.

What to implement:
- Execute `*`, `*min..max`, `*min..`, and `*..max` traversal bounds.
- Add cycle handling policy.
- Add max traversal guardrails to prevent runaway queries.

Files to study/change:
- `core/query/planner.rs`
- `core/query/executor.rs`
- `core/storage/page/record.rs`

Database topics to know first:
- Graph traversal semantics.
- Path uniqueness policies.
- Query limits/guardrails.

DSA/storage topics to know first:
- BFS.
- DFS.
- Visited sets.
- Path expansion.

Rust concepts to know first:
- Queues with `VecDeque`.
- HashSet visited tracking.
- Recursive vs iterative traversal.

Definition of done:
- Variable-length traversal returns correct paths/nodes.
- Traversal has safety limits and tests for cycles.

## [ ] Step 22: Add Concurrency And Isolation

Why this comes after single-threaded correctness:
- Concurrent correctness is hard unless single-writer semantics, recovery, indexes, and storage invariants are already stable.

What to implement:
- Coarse `Arc<RwLock<HiveDb>>` wrapper first.
- Define reader/writer behavior.
- Add lock ordering rules.
- Later add page-level locks and MVCC snapshots.

Files to study/change:
- `core/db/hive_db.rs`
- `core/transaction.rs`
- Public binding crate APIs.

Database topics to know first:
- Isolation levels.
- Reader/writer locks.
- MVCC basics.
- Deadlocks.

DSA/storage topics to know first:
- Lock ordering graphs.
- Snapshot maps/version chains later.

Rust concepts to know first:
- `Arc`.
- `RwLock`.
- `Send` and `Sync`.
- Poisoned lock handling.

Definition of done:
- Concurrent readers are safe.
- Writers are exclusive.
- Recovery/checkpointing remains correct.

## [ ] Step 23: Add Observability And Integrity Tools

Why this comes near the end:
- Debug tools are most useful once storage/index/query invariants are stable.

What to implement:
- Page inspector.
- WAL inspector.
- Query `EXPLAIN`.
- Query timing and row-count metrics.
- Storage integrity checker.
- Index consistency checker.
- Database statistics.

Files to study/change:
- `tools/*`
- `core/storage/*`
- `core/wal/*`
- `core/query/planner.rs`
- `cli/main.rs`

Database topics to know first:
- Integrity checking.
- Query plans.
- Operational diagnostics.

DSA/storage topics to know first:
- Tree validation.
- Graph consistency checks.
- Page graph traversal.

Rust concepts to know first:
- CLI command design.
- Formatting reports.
- Non-mutating inspection APIs.

Definition of done:
- Corrupt or inconsistent storage can be diagnosed.
- Query plans can be printed before execution.
- Index consistency can be verified.

## [ ] Step 24: Continuous Production Test Matrix

Why this is continuous:
- Every previous step must add tests before it is considered complete.

Required commands:

```bash
cargo fmt --check --workspace
cargo clippy --workspace -- -D warnings
cargo check --workspace --all-targets
cargo test --workspace
```

Required test categories:
- Parser tests.
- Planner tests.
- Executor tests.
- Storage layout tests.
- WAL recovery tests.
- Query rollback tests.
- Metadata rollback tests.
- Index consistency tests.
- Graph integrity tests.
- Reopen/recovery tests.
- Randomized full-scan vs indexed comparison tests.

Database topics to know first:
- Crash testing.
- Regression testing.
- Property-based/randomized testing.

DSA/storage topics to know first:
- Model-based testing.
- Consistency invariants.

Rust concepts to know first:
- Test modules.
- Temp directory isolation.
- Deterministic random seeds.
- CI-friendly test design.

Definition of done:
- Every production step adds focused tests.
- Workspace formatting, clippy, check, and tests pass.
