# Hive DB Study Plan

> Updated: July 2026. Page-based storage engine with buffer pooling, page caching, physical WAL, and pager recovery.

---

## What Exists Now

### Page Format

Files: `core/storage/page/format.rs`, `layout.rs`, `record.rs`, `serializer.rs`

- `PAGE_SIZE = 4096`, `META_PAGE_ID = 0`.
- `PageType`, `PageHeader`, `MetaHeader`, `SlotEntry`.
- `is_meta_page()` helper checks Hive magic bytes.
- Slotted page init, insert, read, delete, compact, checksum.
- `NodeRecord`, `EdgeRecord`, `PropertyRecord`.

### Record IDs

File: `core/types.rs`

- `NodeId`, `EdgeId`, `PropertyId` are `u64` type aliases.
- `pack_record_id(page_id, slot_id)` encodes into a single u64.
- `unpack_record_id(id)` decodes back to `page_id + slot_id`.
- Layout: `[page_id: 32 bits][slot_id: 16 bits][flags: 16 bits reserved]`.
- `NIL_ID = u64::MAX`, `is_nil_id()`, `DELETED` flag.

### Buffer Pool

File: `core/storage/buffer_pool.rs`

- Fixed-size pool of reusable 4KB buffers.

### Page Cache

File: `core/storage/page_cache.rs`

- `HashMap<PageId, CachedPage>`, SIEVE eviction, pin/dirty/spilled state.
- Meta page (page 0) pinned automatically on insert.

### Pager

File: `core/storage/pager.rs`

- Opens `hive.db`, owns cache/pool/LSN.
- On first open, bootstraps page 0 with valid `MetaHeader`.
- Reads/writes pages through cache, marks dirty on mutation.
- Allocates new pages by extending file.

### Physical WAL And Recovery

Files: `core/wal/wal_entry.rs`, `wal.rs`, `recovery.rs`, `codec/*`, `utils.rs`

- `WalEntry::Begin`, `PageImage`, `Commit`, `Checkpoint`.
- CRC check, corrupt tail handling.
- Recovery replays committed page images.
- Write path now emits autocommit WAL records: `Begin -> PageImage(s) -> Commit -> sync`.
- Page LSNs are stamped before WAL page images are captured.
- Manual checkpoint flushes dirty pages to `hive.db` and truncates `wal.hive`.
- Automatic checkpointing can truncate WAL after a configured commit interval.
- Rollback frees newly allocated data/overflow pages for reuse in the current pager session.

### HiveDb

File: `core/db/hive_db.rs`

- `HiveDb::open(path)` / `HiveDb::close()`.
- Open invokes WAL recovery.
- Mutating CRUD methods are WAL-backed autocommit operations.
- `HiveDb::begin()` starts an explicit transaction with CRUD methods and `commit()` / `rollback()`.
- `HiveDb::checkpoint()` manually flushes dirty pages and truncates WAL.
- `HiveDb::set_auto_checkpoint_interval(n)` configures automatic checkpoints; `0` disables them.
- `create_node()` — allocates a DataNode page, serializes NodeRecord, returns packed NodeId.
- `get_node(node_id)` — unpacks ID, reads page, deserializes NodeRecord.
- `create_edge(src, dst)` — allocates a DataEdge page, serializes EdgeRecord, returns packed EdgeId.
- `get_edge(edge_id)` — unpacks ID, reads page, deserializes EdgeRecord.
- `set_node_property(node_id, key, value)` — adds/updates a property on a node.
- `get_node_property(node_id, key)` — reads a property value by key.
- Meta page tracks `node_count`, `edge_count`, `root_data_page`, `root_edge_page`.

### Query Layer

Files: `parser/*`, `core/query/planner.rs`, `core/query/result.rs`

- Parser and planner work.
- Executor stubbed.

### Tests

```bash
cargo check -p hive_core_testing --all-targets
cargo test -p hive_core_testing
cargo fmt --check -p hive_core_testing
```

---

## What Is Left To Implement

### 1. WAL Commit Integration

Goal: writes must be recoverable after crash.

Done:
- Autocommit path: Begin -> page LSN stamps -> WAL page images -> Commit -> sync.
- Manual checkpoint path: flush dirty pages into `hive.db`, sync, truncate WAL.
- Rollback/before-image handling for transactional record and metadata pages.
- Engine-generated transaction IDs.
- Public explicit transaction CRUD API (`tx.create_node`, `tx.set_node_property`, etc.).
- Automatic checkpoint policy.
- Reuse of rolled-back newly allocated data and overflow pages.
- Crash-style tests for nodes, edges, properties, checkpoint, and LSN ordering.
- Transaction commit, rollback, and automatic checkpoint tests.
- Rollback page-reuse tests for newly allocated edge and overflow pages.

### 2. Query Executor

Goal: end-to-end query execution.

Tasks:
- `CREATE` node/relationship execution.
- `MATCH` scans/traversals.
- `WHERE`, `RETURN`, `SET`, `DELETE`, `MERGE`.
- Tests after graph CRUD is restored.

### 3. B-Tree Indexes

Goal: durable page indexes for property/label/edge-type lookup.

Tasks:
- Index interior and leaf pages.
- Exact-match lookup.
- Range scans later.

### 4. Advanced Features (after correctness)

- Coarse `Arc<RwLock<HiveDb>>` wrapper.
- Page-level locks.
- Async/background checkpointing.
- MVCC snapshots.
- Page compaction and space reclamation.

---

## Architecture

```text
bindings/rust         public hive crate
core/db               HiveDb open/close, CRUD, properties
core/types            NodeId/EdgeId pack/unpack, NIL_ID, DELETED flag
core/value            Value enum, inline encoding, hash_key
core/storage
  buffer_pool.rs      reusable 4KB buffers
  page_cache.rs       page cache + eviction
  pager.rs            page I/O + cache/pool
  page/format.rs      page headers, meta, types
  page/layout.rs      slotted page operations (insert, update, delete, compact)
  page/record.rs      NodeRecord, EdgeRecord, PropertyRecord
  page/serializer.rs  byte helpers, varints, checksum
core/wal              physical WAL + redo recovery
core/query            parser/planner, executor stubbed
testing/rust          page storage, cache, WAL, bootstrap, CRUD, property tests
```

---

## Definition Of Done

Minimum working database:
- Nodes can be created, read, and reopened from disk.
- Edges can be created, read, and traversed.
- Properties survive reopen.
- WAL recovers committed dirty pages.
- Query executor supports basic `CREATE` and `MATCH`.
- Examples are end-to-end again.

Production-ready:
- B-tree indexes.
- Checkpointing and WAL truncation.
- Freelist and page reuse.
- Long string/overflow pages.
- Page compaction.
- Concurrency and async I/O.

---

## References

1. Database Internals (storage, buffer management, WAL).
2. SQLite page format, btree, WAL docs.
3. PostgreSQL page layout and buffer manager.
4. CMU 15-445 storage, buffer pool, recovery lectures.
5. Turso/Limbo pager, page cache, btree code.
6. Designing Data-Intensive Applications.
