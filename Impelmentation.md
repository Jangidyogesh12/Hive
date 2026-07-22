# Hive Implementation Notes

This document records the implementation work completed from `plan.md` through the phase-8 query surface that is currently practical on the existing pager-backed storage engine.

## Architecture Direction

The code now follows the same broad separation used by mature Rust database engines such as Turso:

- Parser owns syntax and produces AST clauses.
- Planner validates scope and converts clauses into execution steps.
- Executor runs a row/binding pipeline over planned steps.
- Storage mutation stays behind `HiveDb` and `Transaction` APIs.
- Page internals remain isolated in the storage/page modules.

## Parser And AST

- Replaced the old single top-level statement enum with `Statement { clauses: Vec<Clause> }`.
- Added clause pipeline support for `CREATE`, `MATCH`, `WHERE`, `SET`, `DELETE`, `DETACH DELETE`, `MERGE`, and `RETURN`.
- Added comma-separated delete variables with `DELETE n, r`.
- Added phase-8 return modifiers: `ORDER BY`, `ASC`, `DESC`, `SKIP`, and `LIMIT`.
- Kept existing pattern, expression, node, relationship, and return item structures where possible.

## Planner

- Rebuilt `core/query/planner.rs` around executable plan steps.
- Added sequence planning for multi-clause queries.
- Added planning for node create, relationship create, node merge, node scan, edge traversal, filter, set, delete, and return.
- Added variable-scope validation for `WHERE`, `SET`, `DELETE`, and `RETURN` expressions.
- Preserved full-scan index hints so future indexes can replace scans without changing query semantics.
- Explicitly rejects variable-length relationship execution until traversal expansion is implemented.

## Storage APIs

- Added `HiveDb::scan_nodes()` and `HiveDb::scan_edges()` over live slots in `DataNode` and `DataEdge` pages.
- Added `HiveDb::delete_edge()` and transactional `Transaction::delete_edge()`.
- Added safe `HiveDb::delete_node()` and `Transaction::delete_node()`.
- Added `HiveDb::node_has_edges()` so node deletes reject incident relationships unless `DETACH DELETE` removes them first.
- Meta `node_count` and `edge_count` remain allocation counters used for ID generation; deletes do not decrement them.

## Query Executor

- Replaced the stub executor with a row-stream execution engine.
- Added private runtime bindings with node and edge entity references.
- Added expression evaluation for literals, variables, properties, comparisons, boolean `AND`/`OR`, and unary `NOT`.
- Missing properties evaluate to `Value::Null` in query expressions.
- Added `CREATE` for nodes and single-segment relationships with labels/types and properties.
- Added `MATCH` node scans, label filters, property filters, and one-hop outgoing/incoming/undirected traversal.
- Added `WHERE` filtering over row streams.
- Added `RETURN` projection with aliases, derived column names, entity ID projection, ordering, skip, and limit.
- Added `SET` for node and edge properties.
- Added `DELETE` with entity deduplication and edge-before-node ordering.
- Added `DETACH DELETE` by collecting incident edges before deleting nodes.
- Added single-node `MERGE` using full scans over label and property equality.

## Public API And CLI

- Added `HiveDb::execute(query: &str)` as the public parse-plan-execute entrypoint.
- Wired the CLI REPL to execute queries and print returned result tables.
- Updated crate docs to reference the new execution API.

## Tests

Added end-to-end tests for:

- `CREATE ... RETURN`.
- `MATCH ... WHERE ... SET ... RETURN`.
- Relationship traversal with edge property return.
- Single-node `MERGE` reuse.
- Edge delete followed by safe node delete.
- `ORDER BY ... SKIP ... LIMIT`.

Verification commands run successfully:

```bash
cargo fmt --check -p hive_core_testing
cargo check -p hive_core_testing --all-targets
cargo test -p hive_core_testing
cargo check --workspace --all-targets
```

## Remaining Work

The following phase items are scaffolded or partially supported but still need deeper storage work before they can be called production complete:

- Transactional label/property-key dictionaries with rollback-safe before images for label metadata.
- Durable B-tree index pages and index maintenance on insert, update, delete, rollback, and recovery.
- Adjacency-chain maintenance for faster traversal.
- Persistent freelist and record-level reuse beyond current slotted-page delete/reuse behavior.
- Aggregation, `COUNT`, `WITH`, `OPTIONAL MATCH`, label/property `REMOVE`, parameters, lists/maps, and variable-length traversal execution.
- Relationship `MERGE` and uniqueness constraints for production-safe deterministic merge semantics.
