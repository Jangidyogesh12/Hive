# Cypher Support

Hive implements a focused subset of Cypher for local graph creation, traversal, mutation, and projection.

## Values

Supported literal values:

- `NULL`
- Integers: `42`, `-7`
- Floats: `3.14`
- Booleans: `true`, `false`
- Strings: `"Alice"`

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

Find or create a single node by label and properties:

```cypher
MERGE (n:Person {name: "Alice"})
```

Path `MERGE` is not supported yet.

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

Variable-length traversal:

```cypher
MATCH (a)-[:KNOWS*1..3]->(b) RETURN b
```

## RETURN

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

The CLI renders non-empty results as ASCII tables.

## SET

Update a property on a matched binding:

```cypher
MATCH (n:Person {name: "Alice"}) SET n.age = 31
```

## DELETE

Delete a matched binding:

```cypher
MATCH (n:Person {name: "Alice"}) DELETE n
```

Deletes are logical deletes. Records are marked deleted and their IDs can be reused through free lists.

## Known Limitations

- `MATCH` currently requires a `RETURN` clause for read queries.
- Relationship `MERGE` is not supported.
- Planner-level index selection is basic.
- Edge property indexes are not implemented yet.
- The supported syntax is intentionally smaller than Neo4j Cypher.
