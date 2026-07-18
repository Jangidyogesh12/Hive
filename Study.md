# Hive DB Study Plan

> Updated: July 2026. This document reflects the cleanup from the old multi-file node/edge/property stores to the new page-based storage direction with buffer pooling, page caching, physical WAL, and pager recovery.

---

## Current Direction

Hive is being rebuilt around one page-oriented storage engine instead of many separate record files.

Old direction:
- Separate files/stores for nodes, edges, properties, strings, labels, indexes, freelists.
- Fixed-width records written directly at offsets.
- Logical graph APIs like `create_node`, `create_edge`, `get_node`, `info`, and property lookup lived above those stores.
- Query executor assumed those graph APIs existed.

New direction:
- One main database file managed as 4KB pages: `hive.db`.
- A `Pager` owns disk I/O, cache, buffer pool, dirty page tracking, and sync/flush boundaries.
- Page layout supports variable-width records using slotted pages.
- WAL is now physical: full page images, not logical node/edge operations.
- High-level graph CRUD and query execution need to be rebuilt on top of the pager.

---

## What Was Removed Or Made Stale

The old storage model is no longer the source of truth.

Removed/stale concepts:
- Node store file APIs.
- Edge store file APIs.
- Property store file APIs.
- String/label/freelist stores as separate old-format stores.
- Old fixed-width node/edge/property record flow.
- Old logical WAL entries tied to graph operations.
- Old query executor path that expected direct `HiveDb` graph methods.
- Old tests for the deleted store APIs.

Stale public examples were also fixed:
- `examples/social_graph.rs` no longer calls removed APIs.
- `examples/knowledge_graph.rs` no longer calls removed APIs.
- Examples now open a database and demonstrate Cypher parse/plan output.

---

## What Exists Now

### Page Format

Implemented files:
- `core/storage/page/format.rs`
- `core/storage/page/layout.rs`
- `core/storage/page/record.rs`
- `core/storage/page/serializer.rs`

Implemented concepts:
- `PAGE_SIZE = 4096`.
- `PageType` for meta, node data, edge data, property data, string data, label data, index pages, freelist, and overflow.
- `PageHeader` with type, slot count, free-space offset, freeblock pointer, checksum, LSN, and reserved bytes.
- `MetaHeader` for database metadata.
- `SlotEntry` for slotted-page records.
- Page initialization, insert, read, delete, compact, checksum update/verify.
- Variable-width `NodeRecordV2`, `EdgeRecordV2`, and `PropertyRecordV2`.

Status: foundation exists and has tests.

Still needed:
- Stable record ID mapping: decide how `NodeId`/`EdgeId` maps to page id + slot id.
- Page allocation by page type.
- Meta page initialization on first open.
- Free page tracking integrated with allocation.
- Long string/overflow storage integration.

---

### Buffer Pool

Implemented file:
- `core/storage/buffer_pool.rs`

Implemented concepts:
- Fixed-size pool of reusable 4KB buffers.
- `acquire()` and `release()`.
- Avoids allocating a fresh page buffer every time.

Status: simple synchronous buffer pool exists.

Still needed:
- Capacity configuration from database options.
- Metrics/debug counters.
- Better behavior when the pool is exhausted.
- Future concurrency-safe version when page-level locking/async I/O arrives.

---

### Page Cache

Implemented file:
- `core/storage/page_cache.rs`

Implemented concepts:
- `HashMap<PageId, CachedPage>`.
- Clock/SIEVE-style eviction queue.
- Pin count.
- Dirty flag.
- Spilled flag.
- Meta page is protected from eviction.
- Dirty pages are only evictable after they are spilled or flushed.

Status: cache state machine exists and has tests.

Still needed:
- Real WAL spill integration before eviction.
- Better page handles/guards so callers cannot accidentally hold references while eviction occurs.
- Cache statistics and tuning.
- Clear policy for dirty page checkpointing.

---

### Pager

Implemented file:
- `core/storage/pager.rs`

Implemented concepts:
- Opens `hive.db` inside the database directory.
- Owns `FileHandle`, `PageCache`, `BufferPool`, and LSN counter.
- Reads pages through the cache.
- Mutable page access marks pages dirty.
- Allocates new pages by extending file size.
- Flushes/syncs dirty cached pages to disk.
- Exposes direct disk read/write helpers for recovery.

Status: basic pager exists.

Still needed:
- First-open database bootstrap: create and initialize meta page.
- Page-type-aware allocation.
- Freelist reuse instead of only append allocation.
- WAL-before-data rule in commit path.
- Transaction-aware dirty page collection.
- Page LSN updates when writing page images.
- Safer page APIs using read/write guards instead of returning copied arrays or raw mutable refs.

---

### Physical WAL And Recovery

Implemented files:
- `core/wal/wal_entry.rs`
- `core/wal/wal.rs`
- `core/wal/recovery.rs`
- `core/wal/codec/*`
- `core/wal/utils.rs`

Implemented concepts:
- `WalEntry::Begin`.
- `WalEntry::PageImage`.
- `WalEntry::Commit`.
- `WalEntry::Checkpoint`.
- CRC check per WAL record.
- Partial/corrupt tail is ignored during WAL read.
- Recovery replays committed page images if WAL page LSN is newer than disk page LSN.
- `HiveDb::open` invokes recovery.

Status: physical WAL record format and redo recovery exist.

Still needed:
- Actual commit path that writes dirty pages to WAL before disk.
- Checkpoint path that flushes WAL pages into `hive.db` and truncates WAL safely.
- Rollback/subjournal or before-image handling.
- Transaction IDs generated by the engine instead of manually in tests.
- WAL sync policy and durability settings.
- Stronger LSN consistency: page header LSN should be updated before page image logging.

---

### HiveDb Public API

Implemented file:
- `core/db/hive_db.rs`

Current API:
- `HiveDb::open(path)`.
- `HiveDb::close()`.
- Internally owns `Pager` and `Wal`.

Status: only open/close is active.

Removed/not rebuilt yet:
- `create_node`.
- `create_edge`.
- `get_node`.
- `get_edge`.
- `set_property`.
- `lookup_*_by_property`.
- `neighbors`.
- `info`.

Still needed:
- Rebuild graph CRUD on top of pager pages.
- Define durable node/edge/property ID format.
- Maintain adjacency links in `EdgeRecordV2` or via indexes.
- Maintain counts in meta page.
- Expose a clean public API from `bindings/rust` once core APIs stabilize.

---

### Query Layer

Implemented files:
- `parser/*`
- `core/query/planner.rs`
- `core/query/result.rs`

Current status:
- Parser works.
- Planner works.
- Examples demonstrate parse/plan output.
- Executor is intentionally stubbed in `core/query/executor.rs`.

Still needed:
- Rebuild executor against pager-backed graph APIs.
- Implement `CREATE` node/relationship execution.
- Implement `MATCH` scans/traversals.
- Implement `WHERE`, `RETURN`, `SET`, `DELETE`, `MERGE` execution.
- Add tests after graph CRUD is restored.

---

### Tests

Current status:
- Old store tests were removed because they targeted deleted APIs.
- Remaining tests focus on page storage, buffer pool, page cache, serialization, records, layout, and WAL.
- Test module wiring is now test-only via `#[cfg(test)]`, so normal `cargo check` does not compile test modules as library code.

Useful commands:

```bash
cargo check -p hive_core_testing --all-targets
cargo test -p hive_core_testing
cargo fmt --check -p hive_core_testing
```

---

## Recommended Next Work Order

### 1. Finish Database Bootstrap

Goal: opening a new database should create a valid page-based database file.

Tasks:
- On first open, allocate page 0 or page 1 consistently for meta.
- Initialize `MetaHeader` with version, page size, db size, counts, roots, freelist head, and LSN.
- Decide whether page ids start at 0 or 1 and make `META_PAGE_ID` consistent everywhere.
- Add tests for opening a brand-new DB and reopening it.

Why first:
- Every future operation depends on a valid meta page and stable page numbering.

---

### 2. Define Durable Record IDs

Goal: `NodeId` and `EdgeId` must locate records inside pages.

Possible design:

```text
NodeId/EdgeId = packed u64
high 32 bits: page_id
low 16 bits: slot_id
remaining bits: generation/type flags if needed
```

Tasks:
- Add helpers to pack/unpack IDs.
- Decide whether IDs include generation counters to detect deleted/reused slots.
- Add tests for ID encoding and decoding.

Why second:
- Graph APIs cannot be rebuilt until IDs can point to page records.

---

### 3. Rebuild Node CRUD On Pages

Goal: restore `create_node` and `get_node` using `NodeRecordV2` and slotted pages.

Tasks:
- Find or allocate a `DataNode` page with enough free space.
- Encode `NodeRecordV2` into bytes.
- Insert bytes into page layout.
- Return packed `NodeId`.
- Read node by unpacking page id + slot id.
- Update meta node count.
- Add tests for create/read/reopen.

Do not rebuild everything at once. Get nodes working first.

---

### 4. Rebuild Edge CRUD And Adjacency

Goal: restore `create_edge` and traversal basics.

Tasks:
- Store `EdgeRecordV2` in `DataEdge` pages.
- Set `src`, `dst`, `label_id`, and properties.
- Decide adjacency model:
  - linked lists via `first_out_edge`, `first_in_edge`, `next_out_edge`, `next_in_edge`; or
  - page-backed indexes for adjacency.
- If using linked lists, update source and destination node records transactionally.
- Add tests for outgoing, incoming, and undirected traversal.

---

### 5. Rebuild Properties, Labels, And Strings

Goal: make graph records useful beyond raw IDs.

Tasks:
- Decide how property keys are stored: hash only, dictionary, or inline key table.
- Store short values inline using existing `Value::to_inline_bytes`.
- Store long strings in `StringData`/`Overflow` pages.
- Rebuild label storage using page-backed label pages or a B-tree later.
- Add tests for string, integer, float, boolean, null, and long string properties.

---

### 6. Integrate WAL Commit Correctly

Goal: writes must be recoverable after crash.

Tasks:
- Transaction starts with `Begin` WAL entry.
- Dirty pages get page LSNs.
- Dirty page images are appended to WAL.
- WAL is synced before pages are flushed to `hive.db`.
- Commit entry is appended and synced according to durability policy.
- Recovery replays committed page images.
- Add crash-style tests using WAL files and reopened DBs.

---

### 7. Rebuild Query Executor

Goal: examples can become true end-to-end examples again.

Tasks:
- Execute `QueryPlan::CreateNode`.
- Execute `QueryPlan::CreateRelationship`.
- Execute `ScanNodes` using full page scan first.
- Execute `TraverseEdges` using adjacency.
- Execute filters and returns.
- Keep index hints ignored at first, then use them after B-tree indexes exist.

---

### 8. B-Tree Indexes On Pages

Goal: replace old in-memory/hashmap index direction with durable page indexes.

Tasks:
- Implement index interior and leaf pages.
- Exact-match property lookup.
- Label lookup.
- Edge type lookup.
- Later: range scans and statistics.

This should come after basic graph CRUD and full scans work.

---

### 9. Concurrency, Async I/O, MVCC, Compaction

These are later phases after correctness.

Order:
1. Coarse `Arc<RwLock<HiveDb>>` wrapper.
2. Page-level locks inside pager.
3. Async/background checkpointing.
4. MVCC snapshots.
5. Page compaction and space reclamation.

Do not start these until storage, WAL, graph CRUD, and executor basics are correct.

---

## Current Architecture Map

```text
bindings/rust
  public hive crate, currently re-exports hive_core

core/db
  HiveDb open/close only
  old index store stubbed for future B-tree

core/storage
  buffer_pool.rs      reusable 4KB buffers
  page_cache.rs       page cache + eviction metadata
  pager.rs            page I/O + cache/pool coordinator
  page/format.rs      page headers, meta header, page types
  page/layout.rs      slotted page operations
  page/record.rs      NodeRecordV2, EdgeRecordV2, PropertyRecordV2
  page/serializer.rs  byte helpers, varints, checksum

core/wal
  physical WAL entries and redo recovery

core/query
  parser/planner available
  executor stubbed until graph APIs return

testing/rust
  page storage, cache, buffer pool, record, serializer, WAL tests
```

---

## Mental Model

Think of the rebuild like this:

```text
Old Hive:
  HiveDb -> NodeStore/EdgeStore/PropertyStore -> many files

New Hive:
  HiveDb -> Transaction -> Pager -> PageCache -> BufferPool -> hive.db
                         -> WAL -> wal.hive
```

The important shift is that graph concepts are no longer files. Nodes, edges, properties, labels, strings, indexes, and freelist entries are all records inside typed pages.

---

## Definition Of Done For The Rebuild

Minimum working database:
- New DB opens and initializes meta page.
- Nodes can be created, read, and reopened from disk.
- Edges can be created, read, and traversed.
- Properties survive reopen.
- WAL recovers committed dirty pages.
- Query executor supports basic `CREATE` and `MATCH`.
- Examples are end-to-end again.

Production-ready direction:
- Durable indexes on B-tree pages.
- Checkpointing and WAL truncation.
- Freelist and page reuse.
- Long string/overflow pages.
- Page compaction.
- Coarse then fine-grained concurrency.
- Async I/O and MVCC only after the synchronous engine is correct.

---

## References To Study

1. Database Internals, chapters on storage, buffer management, and WAL.
2. SQLite page format, btree, and WAL documentation.
3. PostgreSQL page layout and buffer manager references.
4. CMU 15-445 storage, buffer pool, and recovery lectures.
5. Turso/Limbo pager, page cache, and btree code for Rust design ideas.
6. Designing Data-Intensive Applications for high-level storage engine tradeoffs.
