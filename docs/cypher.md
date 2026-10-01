# Cypher Support

Hive implements a focused subset of Cypher for local graph creation, traversal, mutation, and projection. (Contributor note: how each clause is implemented is documented in `HIVE.md` at the repository root.)

## Values

Supported literal values:

- `NULL`
- Integers: `42`, `-7`
- Floats: `3.14`
- Booleans: `true`, `false`
- Strings: `"Alice"`
- Parameters: `$name` (bound via `HiveDb::execute_with_params` or
  `SharedDb::execute_with_params`; missing parameters are query errors)

Strings must use double quotes.

## CREATE

Create a node:

```cypher
CREATE (n:Person {name: "Alice", age: 30})
```

Create a relationship between two node patterns:

```cypher
CREATE (a:Person {name: "Alice"})-[:KNOWS]->(b:Person {name: "Bob"})
```

Current relationship `CREATE` support expects exactly one relationship segment.

## MERGE

Find or create a single node by label and properties (repeatable without
duplicates; unique constraints are enforced):

```cypher
MERGE (n:Person {name: "Alice"})
```

Relationship `MERGE` matches an edge between resolved endpoints or creates
it deterministically:

```cypher
MERGE (a:Person {name: "Alice"})-[:KNOWS]->(b:Person {name: "Bob"})
```

`ON CREATE SET` runs only when `MERGE` creates; `ON MATCH SET` runs only
when it matches:

```cypher
MERGE (n:Person {email: "a@b.com"})
ON CREATE SET n.created = true
ON MATCH SET n.visits = 2
```

## MATCH

Match nodes by label:

```cypher
MATCH (n:Person) RETURN n
```

Match node properties:

```cypher
MATCH (n:Person {name: "Alice"}) RETURN n
```

Use `WHERE` filters:

```cypher
MATCH (n:Person) WHERE n.age >= 18 RETURN n.name AS name
```

Supported comparison and boolean operators:

```cypher
MATCH (n:Person)
WHERE n.age >= 18 AND NOT n.name = "Anonymous"
RETURN n.name
```

## Relationship Patterns

Directed traversal:

```cypher
MATCH (a)-[:KNOWS]->(b) RETURN a, b
```

Incoming traversal:

```cypher
MATCH (a)<-[:KNOWS]-(b) RETURN a, b
```

Undirected traversal:

```cypher
MATCH (a)-[:KNOWS]-(b) RETURN a, b
```

Chained paths:

```cypher
MATCH (a)-[:KNOWS]->(b)-[:WORKS_AT]->(c) RETURN a, b, c
```

Variable-length traversal (BFS with cycle avoidance; unbounded `*` is
capped at 8 hops):

```cypher
MATCH (a)-[:KNOWS*1..3]->(b) RETURN b
MATCH (a)-[:KNOWS*]->(b) RETURN b
```

## WITH

`WITH` projects rows and replaces the visible scope, enabling multi-stage
pipelines:

```cypher
MATCH (a:Person) WITH a AS x MATCH (x)-[:KNOWS]->(b) RETURN b.name
```

Variables not projected by `WITH` are not visible to later clauses.
`WITH` supports `ORDER BY`, `SKIP`, and `LIMIT` like `RETURN`.

## RETURN and Aggregation

Return full node or edge bindings:

```cypher
MATCH (n:Person) RETURN n
```

Return properties:

```cypher
MATCH (n:Person) RETURN n.name, n.age
```

Return aliases:

```cypher
MATCH (n:Person) RETURN n.name AS person_name
```

Aggregation with `COUNT(*)` / `COUNT(expr)` / `SUM(expr)` / `AVG(expr)` /
`MIN(expr)` / `MAX(expr)` and group-by over the non-aggregate return
expressions:

```cypher
MATCH (n:Person) RETURN COUNT(*)
MATCH (n:Person) RETURN n.city, COUNT(n.city)
MATCH (n:Person) RETURN SUM(n.age), AVG(n.age), MIN(n.age), MAX(n.age)
```

`COUNT(expr)` skips `NULL` values. `COUNT(*)` over an empty input returns
one row with `0`.

`NULL` handling and type rules for the numeric aggregates:

- Every function skips `NULL` values; empty or all-`NULL` groups yield
  `NULL` (except `COUNT(*)`, which yields `0`).
- `SUM` over integers yields `Integer` (overflow is an error); any float
  widens the result to `Float`. Non-numeric values are errors.
- `AVG` always yields `Float`. Non-numeric values are errors.
- `MIN` / `MAX` accept numbers (mixed integers and floats compare
  numerically) or strings (lexicographic) — but never mixed kinds in one
  group, and never booleans, maps, or lists.

Aggregates also work in `WITH` projections:

```cypher
MATCH (n:Person) WITH COUNT(*) AS c RETURN c
MATCH (n:Person) WITH n.city AS city, SUM(n.age) AS total RETURN city, total
```

In an aggregating `WITH`, entity variables evaluate to value maps (a group
has no single entity to pass through). Aggregates anywhere else (e.g.
`WHERE`) are query errors.

## Ordering and Slicing

`RETURN` and `WITH` support `ORDER BY` (with `ASC`/`DESC`), `SKIP`, and
`LIMIT`:

```cypher
MATCH (n:Person) RETURN n.name AS name ORDER BY name DESC LIMIT 10
MATCH (n:Person) RETURN n.name SKIP 5 LIMIT 5
```

`NULL` sorts before all other values.

The CLI renders non-empty results as ASCII tables.

## OPTIONAL MATCH

Row-preserving outer join: input rows without a match are kept with the
new variables bound to null-like values:

```cypher
MATCH (n:Person) OPTIONAL MATCH (n)-[:KNOWS]->(m) RETURN n.name, m.name
```

## SET

Update a property on a matched binding:

```cypher
MATCH (n:Person {name: "Alice"}) SET n.age = 31
```

## REMOVE

Remove a label or a property (transactional and index-safe):

```cypher
MATCH (n:Person) REMOVE n:Employee
MATCH (n:Person) REMOVE n.age
```

## Multiple Labels

Nodes carry one primary label plus extra labels:

```cypher
CREATE (n:Person:Employee {name: "Alice"})
MATCH (n:Person:Employee) RETURN n
```

`RETURN n` includes a `labels` list alongside `label` and `properties`.

## DELETE

Delete a matched binding:

```cypher
MATCH (n:Person {name: "Alice"}) DELETE n
```

Delete incident relationships along with nodes:

```cypher
MATCH (n:Person {name: "Alice"}) DETACH DELETE n
```

Multiple variables can be deleted at once (`DELETE n, r`). Deleting a node
that still has incident edges without `DETACH` is an error and rolls the
query back. Deletes are logical deletes. Records are marked deleted and their IDs can be reused through free lists.

## Observability

In the Rust API, `HiveDb::explain(query)` prints the query plan without
running it. `HiveDb::stats` reports page and record counters;
`check_integrity` and `check_index_consistency` diagnose storage and index
problems. `HiveDb::inspect_page(id)` dumps a byte-level page report
(decoded records, B-tree cells, freelist entries, meta fields);
`HiveDb::inspect_wal(limit)` lists WAL entries oldest-first.
`HiveDb::execute_timed` returns per-stage wall time plus row counts
(`QueryMetrics`: parse, plan, execute). The CLI exposes these as
`.explain <query>`, `.stats`, `.check`, `.inspect <page>`, and
`.wal [limit]`, and prints timing after every query.

## Concurrency

`SharedDb` (`hive_core::db::shared`) pairs a primary handle with an
admission lock: writers are exclusive, while each read-only query opens a
private read-only snapshot (`HiveDb::open_snapshot`) that replays committed
WAL entries into its own cache and never writes the shared files — so
readers run concurrently with each other under snapshot isolation (each
read observes committed state as of its own open). A pooled snapshot cache
is future work.

## Known Limitations

- `MATCH` currently requires a `RETURN` clause for read queries.
- `ORDER BY` cannot reference `RETURN` aliases; repeat the expression instead.
- `WITH` supports value and entity passthrough but not pattern matching.
- Planner-level index selection is basic.
- The supported syntax is intentionally smaller than Neo4j Cypher.
- Release packaging and crates.io publishing are deferred (handled separately).
