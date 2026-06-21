# Storage Format

Hive stores each database as a directory of binary files. The files are local to the process and are opened directly by the Rust engine.

## Files

| File | Purpose |
|---|---|
| `meta.hive` | Database header, magic bytes, version, and record counters |
| `nodes.hive` | Fixed-width node records |
| `edges.hive` | Fixed-width edge records |
| `props.hive` | Fixed-width property records |
| `strings.hive` | Length-prefixed strings for labels, property keys, and long values |
| `labels.hive` | Label/type ID mappings |
| `indexes.hive` | Persisted label, node property, and edge type indexes |
| `wal.hive` | Write-ahead log entries for recovery |
| free-list files | Reusable deleted node and edge IDs |

## Records

Node, edge, and property records are fixed width and addressed by integer IDs. IDs map directly to record positions in their store files.

Nodes store:

- Node ID
- Label ID
- First outgoing edge ID
- First incoming edge ID
- First property ID
- Flags, including the deleted flag

Edges store:

- Edge ID
- Source node ID
- Destination node ID
- Edge type label ID
- Next outgoing edge ID for the source node chain
- Next incoming edge ID for the destination node chain
- First property ID
- Flags, including the deleted flag

Properties store:

- Property ID
- Property key hash
- Property key string offset
- Value type tag
- Inline value bytes or long-string offset
- Next property ID
- Flags

## Strings And Labels

`strings.hive` stores variable-length strings as length-prefixed byte sequences. Property keys and long string values point into this store by offset.

`labels.hive` maintains a bidirectional mapping between label/type strings and compact numeric IDs. Node labels and edge types use these IDs in fixed-width records.

## Property Values

Values are represented by a type tag and a 15-byte inline buffer.

Supported value types:

- Null
- Integer
- Float
- Boolean
- Short string
- Long string

Short strings fit directly in the inline buffer. Long strings are stored in `strings.hive`, and the inline buffer stores the string offset.

## Adjacency

Hive keeps adjacency through linked edge chains instead of separate adjacency lists.

- Each node points to its first outgoing and first incoming edge.
- Each edge points to the next edge in the source node's outgoing chain.
- Each edge points to the next edge in the destination node's incoming chain.

This makes one-hop traversal local to node and edge records while keeping storage append-friendly.

## Indexes

`indexes.hive` persists derived lookup data:

- Node label lookups
- Exact-match node property lookups
- Edge type lookups

Indexes can be loaded from disk or rebuilt from the source-of-truth stores.

## WAL And Recovery

Mutation methods append logical entries to `wal.hive` before changing store files. Checkpoints mark durable boundaries. On open, Hive replays entries after the last checkpoint, rebuilds derived state, writes a clean checkpoint, and truncates the WAL.

The WAL format uses length-delimited entries with type tags and checksums.

## Compatibility

The header stores magic bytes and a version number. `HiveDb::open` rejects unsupported versions to avoid silently interpreting incompatible files.
