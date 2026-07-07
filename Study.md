# Hive DB Study Plan

> **Updated**: July 2026 — restructured with async + multithreading priorities and clear dependency chains.

---

## Dependency Graph

```
Phase 1: Page-Based Storage ──────────────────────────────────────────────────────────────┐
        │                                                                                 │
        ├──▶ Phase 2: Buffer Pool + Page Cache ──────────────────────────────────────────┤
        │         │                                                                       │
        │         ├──▶ Phase 3: Physical WAL + Crash Recovery ────────────────────────────┤
        │         │         │                                                             │
        │         │         ├──▶ Phase 4: Arc<RwLock<HiveDb>> ── Basic Concurrency ───────┤
        │         │         │         │                                                   │
        │         │         │         ├──▶ Phase 5: B-Tree Indexes on Pages ──────────────┤
        │         │         │         │         │                                         │
        │         │         │         │         ├──▶ Phase 6: Page-Level Locking ─────────┤
        │         │         │         │         │         │                               │
        │         │         │         │         │         ├──▶ Phase 7: Async I/O ────────┤
        │         │         │         │         │         │         │                     │
        │         │         │         │         │         │         ├──▶ Phase 8: MVCC ───┤
        │         │         │         │         │         │         │         │           │
        │         │         │         │         │         │         │         │           │
        │         │         │         │         │         │         │         ▼           │
        │         │         │         │         │         │         │   Phase 9: Compaction│
        │         │         │         │         │         │         │         │           │
        │         │         │         │         │         │         │         ▼           │
        │         │         │         │         │         │         │   Phase 10: Query   │
        │         │         │         │         │         │         │   Optimization      │
```

---

## Phase 1: Page-Based Storage (Foundation — Blocks Everything)

Prerequisite for buffering, concurrency, indexes, and compaction. Currently Hive uses fixed-width records (40-byte nodes, 56-byte edges, 56-byte properties) at raw byte offsets. This means:

- No abstraction for caching (buffer pool needs pages as unit of work)
- No efficient multi-record operations (each record is a separate read/write syscall)
- No natural unit for concurrency (can't lock "half a record")
- No alignment with B-tree nodes (which are page-sized)
- Read operations that should be const require `&mut self` because `flush()` forces store mutation

### Resources

- **"Database Internals" Ch. 5-6** — page layout, slotted pages, page headers
- **PostgreSQL page layout** — 8KB pages, tuple format, line pointers
- **SQLite page format** — B-tree page types, cell layout
- **CMU 15-445 L0** — page abstraction fundamentals

### Design Decisions for Hive

- **Page size**: 4KB (OS page size; aligns with filesystem blocks)
- **Slotted page layout** — headers point to variable-length slots within a page
- **Page types** — data pages (nodes/edges/properties), index pages (B-tree interior/leaf), overhead pages (freelist, string overflow, meta page)
- **Multi-record pages** — pack multiple records per page to reduce I/O and enable sequential scans
- **Page header format** — page type, LSN (for WAL recovery), free space offset, slot count, checksum (CRC32)
- **20-byte uniform header** — same size for all page types, simpler than SQLite's 8/12 byte split

### Implementation

1. Define page abstraction (`PageHeader`, `SlotEntry`, `PageType` enum)
2. Implement page-level I/O (file operations on 4KB pages, not individual records)
3. Variable-width record formats (`NodeRecordV2`, `EdgeRecordV2`, `PropertyRecordV2` with varint encoding)
4. Migrate one store at a time (start with nodes, then edges, then properties)
5. Old format compatibility — one-time migration tool or detect format on open

### Files to Create

```
core/storage/page/
├── mod.rs           # Module declarations
├── format.rs        # PageHeader, PageType enum, constants
├── layout.rs        # init_page, insert_record, read_record, delete_record, compact_page
├── record.rs        # Variable-width NodeRecordV2, EdgeRecordV2, PropertyRecordV2
└── serialize.rs     # Varint encode/decode, binary read/write helpers
```

---

## Phase 2: Buffer Pool + Page Cache

Builds on page abstraction — caches pages in memory, reduces disk I/O. Without this, every page read hits the disk even if the page was read 2ms ago.

### Why Both? (They Solve Different Problems)

| | Buffer Pool | Page Cache |
|---|---|---|
| **Analogy** | Warehouse of empty plates | Chef who decides what dish stays on the table |
| **Owns** | Pre-allocated 4KB memory blocks | HashMap `(page_no → PageRef)` + SIEVE eviction queue |
| **Job** | Acquire/free raw memory without `malloc` overhead | Decide which pages to keep in RAM and which to evict |
| **Decoupled** | Swap pool impl (arena, mmap) without touching cache logic | Cache only sees `Arc<Buffer>`, doesn't care where memory came from |

### Buffer Pool Design

- **Simple version** (sufficient for embedded DB): `Vec<Box<[u8; PAGE_SIZE]>>` + `VecDeque<usize>` free list
- `acquire() → Box<[u8; PAGE_SIZE]>` — get an empty page buffer
- `release(Box<[u8; PAGE_SIZE]>)` — return buffer for reuse (data NOT zeroed, just marked free)
- Pre-allocate at startup (e.g., 2000 pages = 8MB)
- **Upgrade path**: Swap for arena-based + lock-free bitmap when io_uring concurrency arrives (Phase 7)

### Page Cache Design

- **Algorithm**: SIEVE (multi-bit improved Clock). Better than LRU for concurrent access.
- **Capacity**: 2000 pages default, configurable
- **Spill threshold**: 90% capacity — proactively spill dirty pages to WAL before cache is critical
- **Eviction rules**:
  - Page 1 (meta page) never evicted
  - Clean pages: evict immediately
  - Dirty pages: spill to WAL first, then evict
  - Pinned pages: skip (pin_count > 0 means actively in use)
- **Intrusive linked list** for eviction queue (avoids per-entry heap allocation)
- O(1) lookup via `HashMap<PageCacheKey, *mut PageCacheEntry>`

### Resources

- **"Database Internals" Ch. 5** — buffer manager design
- **PostgreSQL shared_buffers** — page cache architecture
- **LMDB / BoltDB** — memory-mapped approaches (alternative to explicit buffer pool)
- **InnoDB buffer pool** — production-grade reference implementation

### Files to Create

```
core/storage/buffer_pool.rs    # Arena/buffer allocator
core/storage/page_cache.rs     # SIEVE eviction cache
```

---

## Phase 3: Physical WAL + Crash Recovery

With pages, the WAL switches from logical (entity-level entries like `CreateNode`, `UpdateEdge`) to **physical** (entire page images). This is what SQLite does.

### Why Physical WAL?

| Aspect | Logical WAL (current) | Physical WAL (Phase 3) |
|--------|----------------------|------------------------|
| **Recovery speed** | Must replay every operation sequentially | Copy pages in any order, or leave in WAL |
| **Atomicity** | Must know operation boundaries | Full page is either written or not |
| **Crash at any point** | Complex — need transaction boundaries | Frame-level checksums detect partial writes |
| **Rollback** | Must undo individual operations | Restore page before-image from subjournal |
| **Size** | Small entries | Larger (full 4KB pages) but WAL truncation keeps bounded |

### WAL Frame Format

```
[page_number: u32][db_size: u32][page_data: [u8; 4096]][checksum: u32]
```

### Pager — Page Lifecycle Manager

The `Pager` coordinates all page operations:

```rust
pub struct Pager {
    db_file: File,                  // Main database file (one file of pages)
    wal_file: File,                 // Write-ahead log
    header: DbHeader,               // In-memory copy (page 1 metadata)
    page_cache: PageCache,          // SIEVE cache
    buffer_pool: BufferPool,        // Memory arena
    dirty_pages: RoaringBitmap,     // Set of dirty page IDs
}

impl Pager {
    pub fn read_page(&self, pgno: usize) -> Result<PageRef>;
    pub fn allocate_page(&self, page_type: PageType) -> Result<PageRef>;
    pub fn free_page(&self, pgno: usize) -> Result<()>;
    pub fn add_dirty(&self, page: &PageRef);
    pub fn commit(&self) -> Result<()>;       // Write dirty pages to WAL
    pub fn checkpoint(&self) -> Result<()>;   // Copy WAL pages to main DB
}
```

### Page Lifecycle

```
  ON DISK ──read_page()──▶ LOADED ──modify──▶ DIRTY
                                                │
                                          commit() writes to WAL
                                                │
                                                ▼
                                             SPILLED ──checkpoint()──▶ ON DISK (clean again)
                                                │
                                          can now evict from cache
```

### Subjournal (Savepoints + Rollback)

Before modifying a dirty page, save a **before-image** to the subjournal. On rollback, restore from subjournal entries (reverse order). Replaces the current no-op `rollback()`.

### Resources

- **"Database Internals" Ch. 7-8** — recovery, WAL, ARIES
- **SQLite WAL mode documentation** — how SQLite handles partial writes and crash recovery
- **"Designing Data-Intensive Applications" (Kleppmann) Ch. 3** — storage engine fundamentals

### Files to Create

```
core/storage/pager.rs             # Pager — page lifecycle, I/O coordination
core/storage/subjournal.rs        # Savepoint before-images
core/wal/                         # (refactor existing) — physical frame format
├── wal.rs
└── wal_header.rs
```

---

## Phase 4: Arc\<RwLock\<HiveDb\>\> — Basic Concurrency

**First step toward multithreading.** Simplest to implement, immediately enables concurrent readers.

### Current State (Why This Isn't Possible Yet)

- Every method on `HiveDb` takes `&mut self` (exclusive access)
- `get_node()`, `get_edge()`, `info()` all require `&mut self` because stores call `flush()` before reads — and `BufWriter::flush()` needs `&mut self`
- `Transaction` holds `&mut HiveDb` for its entire lifetime
- Zero synchronization primitives in the codebase

### What Changes After Phases 1-3

With pages and the pager, read operations no longer need `flush()`:
- The pager handles I/O internally via the buffer pool + page cache
- Read-only operations become truly `&self` (they only read pages into the cache)
- Write operations still need exclusive access (WAL append, page modification)

### Implementation

```rust
// Before (current):
let mut db = HiveDb::open(path)?;
let node = db.get_node(5)?;        // &mut self!

// After Phase 4:
let db = Arc::new(RwLock::new(HiveDb::open(path)?));

// Read thread
let db_ref = db.clone();
thread::spawn(move || {
    let db = db_ref.read().unwrap();
    let node = db.get_node(5)?;    // &self — multiple readers OK!
});

// Write thread
let db_ref = db.clone();
thread::spawn(move || {
    let mut db = db_ref.write().unwrap();
    db.create_node("Person", props)?;  // &mut self — exclusive access
});
```

### Concurrency Model

```
                  TIME ──────────────────────────────────────────▶

Reader Thread 1:  [read get_node()                  ]

Reader Thread 2:       [read get_edge()                    ]

Reader Thread 3:            [read info()     ]

Writer Thread:                            [write create_node()]  ← blocks until all readers finish
                                                                   then acquires exclusive lock

Writer done:                                        new readers can proceed
```

### Limitations
- **All writes serialize** — only one writer at a time
- Good enough for **read-heavy workloads** (graph queries, analytics)
- Not good for **write-heavy workloads** (bulk imports) — needs Phase 6

### Rust Concurrency Patterns

- `Arc<RwLock<HiveDb>>` — multi-reader, single-writer
- `Transaction<'_>` borrows `RwLockWriteGuard` — ensures exclusive access during commit
- `HiveDb` must implement `Send + Sync` — enabled by pager using `Arc` internally

### Resources
- **BoltDB's approach** — single-writer with concurrent readers (identical concurrency model)
- **RwLock patterns in Rust** — `Arc<RwLock<T>>` for multi-reader access

### Files to Modify

```
core/db/hive_db.rs            # Method signatures: read ops → &self, write ops → &mut self
core/db/mod.rs                # Re-export
bindings/rust/src/lib.rs      # Public API uses Arc<RwLock<HiveDb>>
cli/main.rs                   # CLI wraps in Arc<RwLock<>>
```

---

## Phase 5: B-Tree Indexes on Pages

With pages, B-tree nodes naturally become single pages. This replaces the current in-memory `HashMap`-based indexes with disk-backed B-tree indexes.

### Current State (In-Memory HashMaps)

- `IndexStore` has four `HashMap`-based indexes (label index, property index, edge type index, edge property index)
- **Persisted to `indexes.hive`** but **rebuilt entirely on every mutation** (O(N) per write)
- Cannot handle datasets larger than RAM
- No range queries (HashMaps only support exact-match lookups)

### B-Tree Design

- **Page types**: `IndexInterior` (routing nodes) and `IndexLeaf` (key-value nodes)
- **Key cells**: Store only `[child_page: u32][key: varint]` — tiny, high fanout
- **Key-Value cells**: Store `[key: varint][value/rowid: varint]` — actual index entries
- **Interior pages fit hundreds of keys** → flat tree (few levels) → fast searches
- **Balance algorithm**: Page split when full, merge when underfull, redistribute among siblings

### Operations
- `btree_search(key) → value` — traverse interior→leaf, binary search within leaf
- `btree_insert(key, value)` — insert into leaf, split if full, propagate splits up
- `btree_delete(key)` — delete from leaf, merge if underfull
- Range scans: walk leaf pages left-to-right using page chain pointers

### Benefits Over Current HashMaps

| | Current HashMap | B-Tree (Phase 5) |
|---|---|---|
| **Memory** | Must fit in RAM | Lives on disk, cached in buffer pool |
| **Writes** | Full rebuild (O(N)) | Incremental insert/delete (O(log N)) |
| **Range queries** | Not supported | Supported (leaf page chains) |
| **Ordered scans** | Not supported | Supported (natural key order) |
| **Rebalance** | Not applicable | Split/merge/redistribute within pages |

### Resources
- **"Database Internals" Ch. 2** — B-tree theory, page splits, rebalancing
- **SQLite btree.c** — gold standard implementation
- **Turso/Limbo `btree.rs`** — Rust production implementation (12,817 lines)

### Files to Create

```
core/storage/btree.rs              # BTreeCursor, balance algorithm, split/merge
core/storage/state_machines.rs     # Cursor state machines (seek, insert, delete, balance)
```

---

## Phase 6: Page-Level Locking — Fine-Grained Concurrency

**Purpose**: Allow multiple writers to modify different pages simultaneously. This is the real performance unlock for write-heavy workloads.

### How It Works

Instead of locking the entire `HiveDb` with a single `RwLock`, each page has its own lock:

```
Phase 4 (coarse):                        Phase 6 (fine-grained):
┌─────────────────────┐                  ┌─────┐ ┌─────┐ ┌─────┐ ┌─────┐
│   RwLock<HiveDb>    │                  │Page1│ │Page2│ │Page3│ │Page4│
│                     │                  │Lock │ │Lock │ │Lock │ │Lock │
│ all pages locked    │                  └──┬──┘ └──┬──┘ └──┬──┘ └──┬──┘
│ as one unit         │                    │       │       │       │
└─────────────────────┘               Writer A  Writer B  Reader C  Free

Writer waits for ALL          Writer A locks only page 1, Writer B locks only page 2
readers & other writers       No conflict — run in parallel!
```

### Implementation

```rust
// Each page has its own RwLock in the pager
pub fn write_page(&self, pgno: usize) -> Result<PageWriteGuard<'_>> {
    let lock = self.page_locks.get(pgno);
    let guard = lock.write().unwrap();
    // ... modify page ...
    Ok(guard)  // lock released on drop
}

pub fn read_page(&self, pgno: usize) -> Result<PageReadGuard<'_>> {
    let lock = self.page_locks.get(pgno);
    let guard = lock.read().unwrap();
    // ... read page ...
    Ok(guard)  // lock released on drop
}
```

### Locking Rules

1. **Read pages**: Acquire read lock — multiple readers OK on same page
2. **Modify pages**: Acquire write lock — exclusive access to that page
3. **B-tree balance**: Locks 2-5 sibling pages (must lock in ascending page number order to prevent deadlocks)
4. **Freelist**: Separate lock for the freelist trunk page (allocation/freeing)
5. **WAL append**: Single writer to the WAL (append-only, sequential by nature)

### Concurrency Model

```
Writer A:  [lock pg5] ── write pg5 ── [unlock] ── [lock pg12] ── write pg12 ── [unlock]

Writer B:     [lock pg8] ── write pg8 ── [unlock]  ← parallel with Writer A!

Reader C:  [lock pg5] ── read pg5 ── [unlock]  ← blocks briefly if Writer A holds pg5
```

### Resources
- **BoltDB** — page-level locking in Go (single writer, concurrent readers per page)
- **InnoDB** — row-level locking with intention locks at the page level

### Files to Modify

```
core/storage/pager.rs       # Add page-level lock map (RwLock per page)
core/db/hive_db.rs          # Remove outer RwLock (now inside pager)
```

---

## Phase 7: Async I/O

**Purpose**: Do disk I/O in the background — checkpoint, WAL sync, page reads — without blocking query execution.

### What Becomes Async?

| Operation | Current (sync) | Async (Phase 7) |
|-----------|----------------|------------------|
| **Read page from disk** | Blocks thread until read(4KB) returns | Issue read, yield thread, resume when OS completes I/O |
| **WAL fsync** | Blocks thread on every commit | Background thread flushes periodically |
| **Checkpoint** | Blocks while copying WAL→DB | Runs in background, DB stays responsive |
| **Flush dirty pages** | Blocks on write | Issue writes, track completions |

### Why Not Start With Async?

- Async adds significant complexity (state machines, IO completion tracking)
- Turso/Limbo uses `io_uring` (Linux-only) — Hive targets macOS/Windows too
- Single-threaded synchronous is correct first, async is optimization after
- Phases 1-6 give correct concurrent behavior; Phase 7 makes it faster

### Approach

Turso/Limbo uses a **linear chain of callbacks** (`IOResult<T>` enum with `Done(T)` and `IO(completions)` variants). The `io_uring` submit/completion loop drives these state machines. Hive can use a simpler model:

```rust
// Simple async via tokio or custom thread pool
impl Pager {
    pub async fn read_page_async(&self, pgno: usize) -> Result<PageRef> {
        if let Some(page) = self.page_cache.get(pgno) {
            return Ok(page);  // Cache hit — sync return
        }
        // Cache miss — async disk read
        let buf = self.buffer_pool.acquire()?;
        self.db_file.read_at(pgno * PAGE_SIZE, &mut buf).await?;
        self.page_cache.insert(pgno, buf);
        Ok(self.page_cache.get(pgno).unwrap())
    }
}
```

### Resources
- **Turso/Limbo `pager.rs`** — async state machines with `IOResult<T>`
- **io_uring** — Linux zero-copy async I/O (Phase 7-advanced)
- **Tokio** — Rust async runtime (Phase 7-basic, cross-platform)

### Files to Create/Modify

```
core/io/
├── mod.rs              # I/O abstraction (sync + async backends)
├── sync_io.rs          # Current synchronous I/O
└── async_io.rs         # Future async I/O (tokio)
```

---

## Phase 8: MVCC — True Concurrent Reads + Writes

**Purpose**: Readers never wait for writers, writers never wait for readers. Each transaction sees a consistent snapshot of the database.

### The Problem MVCC Solves

Even with page-level locking (Phase 6), a writer blocks readers on the same page. MVCC lets readers see the **old version** while the writer creates a **new version** on a different page.

```
WITHOUT MVCC (Phase 6):
  Writer: [modify page 5] ──── DONE
  Reader:          [want page 5... WAIT... OK now read]  ← BLOCKED

WITH MVCC (Phase 8):
  Writer: [copy pg5→pg99] [modify pg99] ── DONE
  Reader: [read pg5 (old version)] ── DONE            ← NO WAIT
          (reader's snapshot points to old page, writer's new page is pg99)
```

### Key Concepts

- **LSN (Log Sequence Number)**: Every page header has an LSN. Every transaction gets a snapshot LSN.
- **Reader's view**: Only pages with LSN ≤ snapshot LSN are visible. Newer pages are invisible.
- **Writer's view**: Creates new page versions with incremented LSN. Old versions stay until no reader needs them.
- **Garbage collection**: Old page versions are freed when the oldest active transaction's snapshot advances past them.

### Version Chain

```
Page 5 version history:
┌─────────┐     ┌─────────┐     ┌─────────┐
│ v1      │────▶│ v2      │────▶│ v3      │  (newest)
│ LSN=10  │     │ LSN=15  │     │ LSN=22  │
│ active  │     │ active  │     │ active  │
│ for txns│     │ for txns│     │ for txns│
│ < LSN15 │     │ < LSN22 │     │ ≥ LSN22 │
└─────────┘     └─────────┘     └─────────┘
     ↑               ↑               ↑
   Reader A       Reader B       Reader C
   (snap=12)      (snap=18)      (snap=30)
   sees v1        sees v2        sees v3
```

### Implementation

```rust
pub struct Transaction {
    snapshot_lsn: u32,          // Which version of the DB this txn sees
    modified_pages: Vec<PageRef>, // New versions created by this txn
}

impl Pager {
    // Read a page as seen by a specific snapshot
    pub fn read_page_for_snapshot(&self, pgno: usize, snapshot_lsn: u32) -> Result<PageRef>;
    
    // Write a page — creates a new version
    pub fn write_page(&self, pgno: usize, txn: &Transaction) -> Result<PageRef>;
}
```

### Resources
- **PostgreSQL MVCC** — tuple-level visibility, xmin/xmax, snapshot isolation
- **InnoDB MVCC** — undo log chains, read views
- **"Database Internals" Ch. 5** — MVCC theory

### Files to Create

```
core/mvcc/
├── mod.rs              # MVCC module
├── transaction.rs      # Transaction with snapshot LSN
├── visibility.rs       # Determine which page version is visible
└── gc.rs               # Garbage collect old page versions
```

---

## Phase 9: Compaction & Space Reclamation

### What Needs Compaction

| Resource | Current Problem | Solution |
|----------|----------------|----------|
| **Data pages** | Deleting records leaves freeblocks and fragmentation | `compact_page()` — move live records together, reset free space offset |
| **String store** | Append-only, strings never deleted, grows forever | Mark-and-sweep GC: identify live strings via property records, rewrite store |
| **WAL** | Grows between checkpoints | Auto-checkpoint: flush to DB file when WAL exceeds threshold (e.g., 1000 pages) |
| **Freelist** | Tracks freed pages | Already works via trunk/leaf linked list; needs integration with page allocator |

### Strategies
- **Lazy compaction**: Compact a page only when free space falls below a threshold (e.g., < 20% usable)
- **Background compaction**: Run in a background thread (after Phase 7 async I/O)
- **Online compaction**: Don't block queries during compaction — compact a page by copying live records to a new page, then swap the page atomically

### Resources
- **LSM compaction strategies** — leveled, tiered (alternative approach)
- **Free-space management in filesystems** — buddy allocator, slab allocator
- **PostgreSQL VACUUM** — how they handle dead tuple cleanup

---

## Phase 10: Query Optimization

- **Cost-based optimization** — PostgreSQL's planner
- **Predicate pushdown** — filter as early as possible (at the buffer pool level)
- **Join ordering** — choose the best traversal order for multi-hop queries
- **Query plan caching** — cache compiled plans for repeated queries
- **Index selection** — choose the best index based on statistics (cardinality, selectivity)

---

## Summary: Action Items (Dependency Order)

| # | Phase | Depends On | Unlocks |
|---|-------|------------|---------|
| 1 | Page-Based Storage | — | Everything below |
| 2 | Buffer Pool + Page Cache | Phase 1 | Caching, WAL integration |
| 3 | Physical WAL + Crash Recovery | Phase 2 | Durability, rollback |
| 4 | Arc\<RwLock\<HiveDb\>\> | Phase 3 | **Concurrent readers** |
| 5 | B-Tree Indexes on Pages | Phase 3 | Range queries, incremental index |
| 6 | Page-Level Locking | Phase 4 | **Concurrent writers** |
| 7 | Async I/O | Phase 6 | Background checkpoint, non-blocking I/O |
| 8 | MVCC | Phase 7 | **Readers never block on writers** |
| 9 | Compaction | Phase 6 | Space reclamation |
| 10 | Query Optimization | Phase 5 | Cost-based plans, predicate pushdown |

## Resources

1. **"Database Internals"** — storage internals (page layout Ch. 5-6, buffer pool Ch. 5, recovery Ch. 7-8)
2. **"Designing Data-Intensive Applications"** — broader systems perspective (Kleppmann)
3. **CMU 15-445** (free on YouTube) — Andy Pavlo's database course
4. **SQLite source code** — `btree.c` and `wal.c` (gold standard for embedded DBs)
5. **LMDB / BoltDB source** — embedded DBs with memory-mapped approaches
6. **PostgreSQL source** — `bufpage.h`, `page.h` for page layout reference
7. **Turso/Limbo source** — Rust SQLite rewrite; `pager.rs`, `btree.rs`, `page_cache.rs` (reference implementation)
8. **Hive Understanding.md** — detailed page format design with architectural diagrams
