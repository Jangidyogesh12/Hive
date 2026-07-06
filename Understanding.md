# Understanding Pages: From Turso/Limbo to Hive DB

> **Goal**: Understand how a production-grade Rust database (Turso/Limbo — a SQLite rewrite) implements page-based storage, and design Hive DB's page format for production-level query execution.

---

## Table of Contents

1. [What is a "Page"? (The Filing Cabinet Analogy)](#1-what-is-a-page-the-filing-cabinet-analogy)
2. [How Turso/Limbo Implements Pages](#2-how-tursolimbo-implements-pages)
   - [2.1 The On-Disk Format](#21-the-on-disk-format)
   - [2.2 The Slotted Page Layout](#22-the-slotted-page-layout)
   - [2.3 Page Types and B-Trees](#23-page-types-and-b-trees)
   - [2.4 The Pager — Page Lifecycle Manager](#24-the-pager--page-lifecycle-manager)
   - [2.5 Page Cache — The SIEVE Algorithm](#25-page-cache--the-sieve-algorithm)
   - [2.6 Buffer Pool — Memory Arena](#26-buffer-pool--memory-arena)
   - [2.7 Write-Ahead Log (WAL) Integration](#27-write-ahead-log-wal-integration)
   - [2.8 Putting It All Together: Read & Write Flow](#28-putting-it-all-together-read--write-flow)
3. [Designing Hive DB's Page Format](#3-designing-hive-dbs-page-format)
   - [3.1 Why Hive Needs Pages Now](#31-why-hive-needs-pages-now)
   - [3.2 The Gap: Current Hive vs. Production Hive](#32-the-gap-current-hive-vs-production-hive)
   - [3.3 Hive's Page Types](#33-hives-page-types)
   - [3.4 Hive's Page Header Design](#34-hives-page-header-design)
   - [3.5 Hive's Slotted Page Layout](#35-hives-slotted-page-layout)
   - [3.6 Record Formats Within Pages](#36-record-formats-within-pages)
   - [3.7 Why This Design? Design Rationale](#37-why-this-design-design-rationale)
4. [Architecture & System Diagrams](#4-architecture--system-diagrams)
   - [4.1 High-Level System Architecture](#41-high-level-system-architecture)
   - [4.2 Component Interaction Diagram](#42-component-interaction-diagram)
   - [4.3 Page Read Flow (Detailed)](#43-page-read-flow-detailed)
   - [4.4 Page Write Flow (Detailed)](#44-page-write-flow-detailed)
   - [4.5 WAL and Recovery Flow](#45-wal-and-recovery-flow)
5. [Production Implementation Plan](#5-production-implementation-plan)
   - [5.1 Phase 1: Page Abstraction & On-Disk Format](#51-phase-1-page-abstraction--on-disk-format)
   - [5.2 Phase 2: Buffer Pool](#52-phase-2-buffer-pool)
   - [5.3 Phase 3: Page Cache](#53-phase-3-page-cache)
   - [5.4 Phase 4: Pager & WAL Integration](#54-phase-4-pager--wal-integration)
   - [5.5 Phase 5: B-Tree Indexes on Pages](#55-phase-5-b-tree-indexes-on-pages)
6. [Key File References](#6-key-file-references)

---

## 1. What is a "Page"? (The Filing Cabinet Analogy)

Imagine you work at a hospital records department:

### The Old Way (Hive Today)
- **Every patient record is a single sheet of paper.**  
- To find patient #5000, you walk to the shelf, count 5000 sheets from the start, and pull out sheet #5000.  
- If you need 10 patients, you make 10 separate trips to the shelf.  
- If you want to add a new patient, you walk to the end and add one sheet.  

This is **random access by record index** — what Hive does today with its `NodeStore`, `EdgeStore`, and `PropertyStore`. Every read is: `seek to (id * 40 bytes), read 40 bytes`. This works for a small clinic. It does NOT work for a city-wide hospital.

### The New Way (Page-Based Storage)
- **Records are grouped into binders (pages) of 4KB each.**  
- Each binder has a table of contents at the front saying: "Patient A starts at slot 1, Patient B at slot 2..."  
- To find patient #5000, you grab binder #125 (since 40 records fit per page) and look at its table of contents.  
- Adding a patient? You find a binder with empty space, write the record in an empty slot, update the table of contents.  
- You keep the most-used 100 binders on your desk (buffer pool). The rest stay on the shelf (disk).  
- When you modify a binder, you write down what you changed in a logbook (WAL) BEFORE you put the binder back. If the power goes out, the logbook tells you what to redo.

**Why this matters for databases:**
- **Fewer disk trips**: Reading binder #125 gives you 40 patients, not just one. Great for scans.
- **Caching**: Keep hot binders in RAM. Only fetch from disk when needed.
- **Concurrency**: Two doctors can work on different binders at the same time. Lock per-binder, not per-record.
- **Crash safety**: The logbook (WAL) records changes at the binder level. On recovery, replay the logbook.
- **B-tree indexes**: B-tree nodes are exactly one binder each. This makes indexes naturally page-aligned.

---

## 2. How Turso/Limbo Implements Pages

Turso/Limbo is a full Rust reimplementation of SQLite. It implements the SQLite page format faithfully. Let's walk through every piece.

### 2.1 The On-Disk Format

The database file is a flat sequence of pages:

```
┌──────────┬──────────┬──────────┬─────┬──────────┐
│  Page 1  │  Page 2  │  Page 3  │ ... │  Page N  │
│ (header) │          │          │     │          │
└──────────┴──────────┴──────────┴─────┴──────────┘
     ↑                                           ↑
  100-byte DB header                      Regular data page
  lives at top of page 1
```

Every page is the same size (configurable: 512B to 64KB, power of 2). Turso defaults to 4KB.

**File**: `core/storage/sqlite3_ondisk.rs:4-43`

The first page is special — its first 100 bytes are the `DatabaseHeader`:

```
Page 1 Layout:
┌──────────────────────────┬──────────────────────────────────────┐
│   DatabaseHeader (100B)  │   B-Tree page content (rest of 4KB) │
│                          │   (this is the sqlite_schema table) │
└──────────────────────────┴──────────────────────────────────────┘
    0                   99  100                             4095
```

The `DatabaseHeader` struct (`sqlite3_ondisk.rs:308-359`) contains:
- Magic string: `"SQLite format 3\0"` (16 bytes)
- Page size (2 bytes)
- Database size in pages (4 bytes)
- Freelist page pointers
- Schema cookie, encoding, version numbers
- **100 bytes total**

**Why 100 bytes?** SQLite defined the header this way decades ago. Every tool that reads SQLite files knows this number. Turso keeps it for compatibility. For Hive, we'll design our own header.

### 2.2 The Slotted Page Layout

This is the most important concept. Every page (except page 1's header region) uses a **slotted page layout**:

```
┌──────────────────────────────────────────────────────────────────┐
│                      A SINGLE 4KB PAGE                          │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐           ┌──────────────┐  │
│  │ Page Header  │  │ Cell Pointer │           │ Cell Content │  │
│  │ (8 or 12 B)  │  │    Array     │  FREE     │     Area     │  │
│  │              │  │ (grows →→→)  │  SPACE    │  (←←← grows) │  │
│  └──────────────┘  └──────────────┘           └──────────────┘  │
│  0              7  8          ...                          4095 │
└──────────────────────────────────────────────────────────────────┘
```

**Two regions grow toward each other:**
- **Cell pointer array** (starts at byte 8 for leaf pages, byte 12 for interior pages): Grows **upward** as cells are added. Each entry is a 2-byte offset pointing to where the cell's actual data lives in the content area.
- **Cell content area** (starts at the end of the page): Grows **downward** as cells take space.

When they meet in the middle, the page is full. This dual-growing design prevents wasted space — content takes exactly the bytes it needs.

**File**: `core/storage/sqlite3_ondisk.rs:17-25`

### 2.3 Page Types and B-Trees

SQLite uses a B-tree to organize data. B-tree nodes are pages. There are 4 page types:

| Type | Code | What it holds |
|------|------|---------------|
| `TableInterior` | `0x05` | B-tree interior node for tables. Points to child pages + rowid keys. |
| `TableLeaf` | `0x0D` | B-tree leaf node for tables. Contains actual row data. |
| `IndexInterior` | `0x02` | B-tree interior node for indexes. Points to child pages + index keys. |
| `IndexLeaf` | `0x0A` | B-tree leaf node for indexes. Contains index keys + rowids. |

**File**: `core/storage/sqlite3_ondisk.rs:504-511`

The page type determines the page header size:
- **Interior pages** = 12-byte header (includes a rightmost pointer)
- **Leaf pages** = 8-byte header (no rightmost pointer needed)

#### B-Tree Page Header (btree.rs offset module, lines 72-120)

```
┌──────┬─────────────────┬────────────┬──────────────────┬──────────┬──────────────┐
│ Type │ First Freeblock │ Cell Count │ Cell Content     │ Frag.    │ Right-most   │
│ (u8) │    (u16)        │   (u16)    │ Area Start (u16) │ Bytes(u8)│ Pointer(u32) │
└──────┴─────────────────┴────────────┴──────────────────┴──────────┴──────────────┘
  0       1            3             5                    7           8         11
  ↑                                                                   ↑
  All pages                                                           Interior pages only
```

**Key fields:**
- **Page Type**: One of the 4 types above.
- **First Freeblock**: Offset to a chain of free space blocks *within* the cell content area (from deleted cells). 0 = no freeblocks.
- **Cell Count**: How many cells are on this page.
- **Cell Content Area**: Offset to the first cell's content. This is where cells get written (from the bottom up). A value of 0 means 65536 (needed for 64KB pages).
- **Fragmented Bytes**: Tiny gaps (1-3 bytes) that can't be used. Gets defragmented when it accumulates.
- **Rightmost Pointer**: Only on interior pages. Points to the child page that contains values greater than the last key on this page.

#### B-Tree Cell Formats

Cells are the actual data containers. Their format depends on the page type:

**Table Leaf Cell** — stores a database row:
```
[payload_size: varint][rowid: varint][payload bytes][overflow_ptr: u32?]
```

**Table Interior Cell** — points to a child page:
```
[left_child_page: u32][rowid: varint]
```

**Index Leaf Cell** — stores an index entry:
```
[payload_size: varint][payload bytes][overflow_ptr: u32?]
```

**Index Interior Cell** — points to a child page with index key:
```
[left_child_page: u32][payload_size: varint][payload bytes][overflow_ptr: u32?]
```

**What are varints?** A variable-length integer encoding (1-9 bytes). Small numbers (0-127) take 1 byte. Up to 2^56-1 can be encoded in 1-8 bytes. The 9th byte is a raw extension for the full 64-bit range. This saves space since most rowids and payload sizes are small.

**File**: `core/storage/sqlite3_ondisk.rs:773-812`

**Why two cell formats (interior vs leaf)?** Interior nodes only need to route you to the right child page — they don't store full row data. Leaf nodes store the actual rows. This separation means interior nodes are much smaller (higher fanout = fewer levels = faster searches).

### 2.4 The Pager — Page Lifecycle Manager

The **Pager** is the central coordinator. Think of it as the **hospital records department manager** — it knows where every binder is (in RAM, on disk, or in the WAL), whether it's clean or dirty, and manages the process of fetching and writing.

**File**: `core/storage/pager.rs`

#### Page Representation

```rust
pub struct PageInner {
    flags: AtomicUsize,          // LOCKED | DIRTY | LOADED | SPILLED
    id: usize,                   // Page number
    pin_count: AtomicUsize,      // How many things are using this page
    wal_tag: AtomicU64,          // Which WAL frame this version came from
    buffer: Option<Arc<Buffer>>, // The actual 4KB of data (None = not loaded)
    overflow_cells: Vec<OverflowCell>,
}
pub struct Page {
    inner: UnsafeCell<PageInner>, // Interior mutability
}
pub type PageRef = Arc<Page>;     // Shared across threads
```

**File**: `core/storage/pager.rs:111-130`

**Key design choices:**
- `UnsafeCell<PageInner>`: Allows interior mutability without locks for page contents. The cache and pager coordinate access at a higher level.
- `Arc<Page>`: Multiple cursors and state machines can hold references to the same page.
- `pin_count`: If > 0, the page cannot be evicted from the cache. Nested operations can each pin independently (increment), and the page stays until ALL of them unpin.

#### Page Flags

| Flag | Meaning |
|------|---------|
| `PAGE_LOCKED` | Currently being read from disk or written to WAL |
| `PAGE_DIRTY` | Modified in memory, needs to be written to WAL/disk |
| `PAGE_LOADED` | Buffer is filled with valid data |
| `PAGE_SPILLED` | Dirty page has been written to WAL early (can evict) |

**File**: `core/storage/pager.rs:735-741`

**Why these flags?** They represent the lifecycle of a page:
```
       +----------+
       | ON DISK  |  (no flags set)
       +----+-----+
            |
            | read_page()
            v
       +----------+
       | LOADED   |  (PAGE_LOADED)
       +----+-----+
            |
            | modify data, mark dirty
            v
       +----------+
       | DIRTY    |  (PAGE_LOADED | PAGE_DIRTY)
       +----+-----+
            |
            | commit (write to WAL)
            v
       +----------+
       | SPILLED  |  (PAGE_LOADED | PAGE_DIRTY | PAGE_SPILLED)
       +----+-----+  Can now be evicted from cache!
            |
            | checkpoint (flush to main DB file)
            v
       +----------+
       | ON DISK  |  (back to clean)
       +----------+
```

#### Key Pager Operations

**Reading a page** (`read_page`, line 3182):
1. Check `pending_reads` — is another thread already fetching this page? If so, share the in-flight request.
2. Check `page_cache.get()` — is it already in memory? Return immediately.
3. Cache miss → `read_page_no_cache()`:
   - Try WAL first (newer version might be there)
   - Fall back to main database file
   - Insert into `page_cache`

**Allocating a new page** (`allocate_page`, line 5104):
1. Check freelist (reuse deleted pages)
2. If freelist empty, extend the file (increment `database_size`)
3. Return a zeroed 4KB page

**Freeing a page** (`free_page`, line 4874):
1. Add to freelist trunk/leaf linked list
2. Page can be reused later

### 2.5 Page Cache — The SIEVE Algorithm

The page cache keeps frequently used pages in memory. Turso uses **SIEVE**, an improved version of the classic Clock/Second-Chance algorithm.

**File**: `core/storage/page_cache.rs`

#### How SIEVE Works (The Library Book Analogy)

Imagine a tiny library desk that holds only 10 books:
1. When you check out an 11th book, you must return one.
2. Each book has a sticky note with a counter (0-3). Read the book? Bump the counter.
3. The librarian keeps her finger on one book (the **clock hand**).
4. To evict (return) a book:
   - Look at the book under her finger.
   - If its counter > 0: decrement it by 1, move finger to next book, repeat.
   - If its counter == 0: that book gets returned! New book takes its spot.
   - Finger moves to the next book after the one just returned.
5. Hot books keep getting their counter bumped, so they survive longer.

**Why SIEVE over LRU?** Simple LRU needs a full linked-list reordering per access (O(n) worst case with intrusive lists). SIEVE just bumps a counter — it tracks "recency in batches" rather than exact recency. This is much faster under concurrent access.

**Why SIEVE over CLOCK?** CLOCK uses a single bit (0/1). SIEVE uses a multi-bit counter (0-3) that gives finer granularity — a page accessed 4 times survives longer than one accessed once. This reduces the "one-time scan flushes entire cache" problem.

#### Key Cache Details

```rust
pub struct PageCache {
    capacity: usize,                                  // Max pages
    map: HashMap<PageCacheKey, *mut PageCacheEntry>,   // O(1) lookup
    queue: LinkedList<EntryAdapter>,                   // Eviction order
    clock_hand: *mut PageCacheEntry,                   // SIEVE finger
    spill_threshold: usize,                            // 90% of capacity
    spill_enabled: bool,
    evictable_count: usize,                            // How many CAN be evicted
}
```

**File**: `core/storage/page_cache.rs:99-113`

- **Page 1 is never evicted**: The database header is always needed.
- **Dirty pages must spill first**: Before eviction, dirty pages are written to WAL (spilled) so the change is durable. Only spilled pages can leave the cache.
- **Pinned pages cannot be evicted**: `pin_count > 0` means something is actively using the page.

#### The Spill Threshold

When the cache hits 90% capacity, the pager proactively spills dirty pages to WAL. This ensures:
1. There's always room for new pages.
2. Spilling happens in the background, not in the critical path of reads.
3. Dirty pages accumulate in WAL at a predictable rate (good for checkpoint scheduling).

### 2.6 Buffer Pool — Memory Arena

Instead of calling `malloc` for every 4KB page (which fragments memory and causes allocation overhead), Turso uses an **arena-based buffer pool**.

**File**: `core/storage/buffer_pool.rs`

#### How an Arena Works (The Parking Lot Analogy)

Picture a huge parking lot:
- The lot is pre-allocated (e.g., 8000 slots × 4KB = 32MB).
- Each slot = exactly one page buffer.
- When you need a page, you take the next available slot. When you're done, mark it free.
- No individual `malloc`/`free` calls. No memory fragmentation.

```rust
pub struct BufferPool {
    inner: UnsafeCell<PoolInner>,  // Thread-safe interior mutability
}

struct PoolInner {
    arenas: Vec<Arena>,            // Multiple large memory regions
    // Each arena has a lock-free bitmap tracking free/used slots
}
```

**Key benefits:**
1. **No per-page allocation**: The pool pre-allocates and reuses memory.
2. **Lock-free slot allocation**: Uses `AtomicSlotBitmap` for fast slot acquisition.
3. **io_uring integration**: The arena can be registered with io_uring for zero-copy async I/O.
4. **Drop-recycling**: When an `ArenaBuffer` is dropped, its slot returns to the pool.

**For Hive**: We'll implement a simpler version — a fixed-size pre-allocated `Vec<Box<[u8; PAGE_SIZE]>>` with a `VecDeque` of free indices. Read from `deque.pop_front()`, return via `deque.push_back(index)`. This is sufficient for an embedded DB without io_uring.

### 2.7 Write-Ahead Log (WAL) Integration

The WAL is the durability mechanism. Its relationship with pages is crucial.

#### WAL File Format

```
┌─────────────────┬───────────────────┬─────┬───────────────────┐
│   WAL Header    │    Frame 1        │ ... │    Frame N        │
│   (32 bytes)    │ (24 + page bytes) │     │ (24 + page bytes) │
└─────────────────┴───────────────────┴─────┴───────────────────┘
```

Each frame:
```
┌──────────────┬────────────┬────────┬────────┬───────────┬───────────┬──────────────┐
│ Page Number  │ DB Size    │ Salt 1 │ Salt 2 │ Checksum 1│ Checksum 2│ Page Data    │
│   (u32)      │ (u32)      │ (u32)  │ (u32)  │  (u32)    │  (u32)    │ (page bytes) │
└──────────────┴────────────┴────────┴────────┴───────────┴───────────┴──────────────┘
     24-byte header                                                         4KB
```

**File**: `core/storage/sqlite3_ondisk.rs:2053`

#### How Pages Flow Through WAL

```
APPEND OPERATION:

 mutate page in RAM
        │
        ▼
 mark page DIRTY
        │
        ▼
 commit(): 
   prepare WAL frame → [header + whole page image + checksum]
        │
        ▼
   write frame to WAL file
        │
        ▼
   fsync(WAL file)           ← durability point
        │
        ▼
   mark page SPILLED         ← can now evict from cache


CHECKPOINT (moves WAL pages to main DB):

 scan dirty pages
        │
        ▼
 read their WAL frames
        │
        ▼
 write pages to main DB file
        │
        ▼
 fsync(main DB file)
        │
        ▼
 update WAL header (advance checkpoint pointer)
        │
        ▼
 truncate WAL (optional)
```

**Key insight**: The WAL stores **entire page images** (physical redo), not logical operations (like "insert node #42"). This is called **physical logging** and it's what SQLite/Turso does.

**Why physical logging?**
- **Simple recovery**: Just copy the page from WAL to the database file. No need to understand the operation semantics.
- **Atomic**: Either a full page is written or it isn't. No half-written logical operations.
- **Fast WAL writes**: Just memcpy the page + append. No encoding/decoding.

**Hive's current WAL is logical** (entries like `CreateNode`, `UpdateEdge`). For production, we should move to **page-level WAL** for the same reasons SQLite does.

### 2.8 Putting It All Together: Read & Write Flow

#### Read Flow (simplified)

```
Application asks for data at rowid X
    │
    ▼
┌─────────────┐     ┌──────────────┐     ┌───────────────┐     ┌──────────┐
│ BTreeCursor │───▶│ P.read_page()│────▶│ page_cache    │────▶│ return   │
│ .seek(X)    │    │              │     │ .get(page_no) │     │ PageRef  │
└─────────────┘    └──────┬───────┘     └───────┬───────┘     └──────────┘
                          │                     │
                          │ CACHE MISS          │ CACHE HIT ──▶ done
                          ▼                     │
                   ┌──────────────┐             │
                   │ Check WAL    │             │
                   │ first?       │             │
                   └──────┬───────┘             │
                          │                     │
                   ┌──────▼───────┐             │
                   │ read from    │             │
                   │ main DB file │             │
                   │ (async I/O)  │             │
                   └──────┬───────┘             │
                          │                     │
                   ┌──────▼───────┐             │
                   │ insert into  │─────────────┘
                   │ page_cache   │
                   └──────────────┘
```

#### Write Flow (simplified)

```
Application inserts a row
    │
    ▼
┌─────────────┐
│ BTreeCursor │  find target leaf page
│ .insert()   │
└──────┬──────┘
       │
       ▼
 page loaded in RAM, cell inserted into page's cell content area
       │
       ▼
 mark page DIRTY
       │
       ▼ (if page overflows → balance → split → new pages marked DIRTY)
       │
       ▼
┌──────────────┐
│ commit()     │
└──────┬───────┘
       │
       ▼
┌──────────────────┐
│ prepare WAL      │
│ frames for all   │
│ dirty pages      │
└──────┬───────────┘
       │
       ▼
┌──────────────────┐
│ write frames to  │
│ WAL file + fsync │
└──────┬───────────┘
       │
       ▼
 mark pages SPILLED
       │
       ▼
 DONE (checkpoint happens later/async)
```

---

## 3. Designing Hive DB's Page Format

Now we design Hive's page format, learning from Turso's implementation while adapting it for a **graph database** rather than a relational one.

### 3.1 Why Hive Needs Pages Now

Hive currently stores data as **fixed-width flat files**:

```
nodes.hive:
┌──────────┬──────────┬──────────┬──────────┐
│ Node #0  │ Node #1  │ Node #2  │ Node #3  │ ... (40 bytes each)
└──────────┴──────────┴──────────┴──────────┘

edges.hive:
┌──────────┬──────────┬──────────┬──────────┐
│ Edge #0  │ Edge #1  │ Edge #2  │ Edge #3  │ ... (56 bytes each)
└──────────┴──────────┴──────────┴──────────┘
```

This is **simple but scales poorly**:

| Problem | Impact |
|---------|--------|
| **O(N) scans** | Every full-table scan reads the entire file, record by record, each requiring a `seek()` + `read()` |
| **No caching** | Hive re-reads the same records from disk repeatedly during traversal |
| **No efficient indexing** | Indexes are in-memory HashMaps. At scale they don't fit in RAM. B-trees require pages. |
| **No concurrency primitives** | You can't lock a "range of a file" — you lock the whole file or nothing. |
| **No compaction** | Deleted records leave holes. Free lists help reuse IDs but physical space is never reclaimed. |
| **WAL is logical** | If the WAL has 10,000 logical entries, recovery replays ALL of them sequentially. Page-level WAL lets recovery copy pages directly. |

### 3.2 The Gap: Current Hive vs. Production Hive

```
CURRENT HIVE                          PRODUCTION HIVE (target)
═════════════                         ════════════════════════

Flat files per entity type          Unified page-based DB file (or few files)
Fixed-width records                  Variable-width records in slotted pages
Random I/O per record               Batched I/O per page
In-memory HashMaps for indexes      B-Tree indexes on pages
Logical WAL entries                 Physical page-level WAL
No buffer pool                      Arena-based buffer pool
No eviction                         SIEVE page cache
Full index rebuild on every write   Incremental B-Tree maintenance
Single-threaded                     Page-level concurrency ready
```

### 3.3 Hive's Page Types

Unlike SQLite (which has 4 types: table interior/leaf + index interior/leaf), Hive needs different page types because it stores a **graph**, not tables:

```
┌─────────────────────────────────────────────────────────────────────┐
│                     Hive's Page Type Taxonomy                       │
├──────────────┬──────────────────────────────────────────────────────┤
│ Page Type    │ Purpose                                              │
├──────────────┼──────────────────────────────────────────────────────┤
│ Meta         │ Database header (one per DB). Replaces meta.hive.    │
│              │ Contains magic, version, counters, root page ptrs.   │
├──────────────┼──────────────────────────────────────────────────────┤
│ DataNode     │ Stores node records in slotted format.               │
│              │ Each slot = one NodeRecord (variable width now).     │
├──────────────┼──────────────────────────────────────────────────────┤
│ DataEdge     │ Stores edge records in slotted format.               │
│              │ Each slot = one EdgeRecord (variable width now).     │
├──────────────┼──────────────────────────────────────────────────────┤
│ DataProperty │ Stores property records in slotted format.           │
├──────────────┼──────────────────────────────────────────────────────┤
│ StringData   │ Stores variable-length strings (was strings.hive).   │
│              │ Large strings span multiple pages.                   │
├──────────────┼──────────────────────────────────────────────────────┤
│ LabelData    │ Stores label<->id mappings (was labels.hive).        │
├──────────────┼──────────────────────────────────────────────────────┤
│ IndexInterior│ B-tree interior node for any index.                  │
├──────────────┼──────────────────────────────────────────────────────┤
│ IndexLeaf    │ B-tree leaf node for any index.                      │
├──────────────┼──────────────────────────────────────────────────────┤
│ Freelist     │ Tracks which pages are free for reuse.               │
│              │ Linked list of page numbers.                         │
├──────────────┼──────────────────────────────────────────────────────┤
│ Overflow     │ When a record is too large to fit in one page.       │
│              │ Chains to continuation pages.                        │
└──────────────┴──────────────────────────────────────────────────────┘
```

**Why separate DataNode and DataEdge pages instead of one unified Data page?**
- **Traversal locality**: When you traverse edges from a node, consecutive edges of that node live together on the same page. If nodes and edges shared pages, a node and its edges would be scattered.
- **Different access patterns**: Nodes are looked up by ID or scanned by label. Edges are traversed in chains. Different page types let us optimize layout per access pattern.
- **Simpler free-space management**: Node pages know their node-to-slot mapping. Edge pages know their edge chains. Mixing them complicates both.

### 3.4 Hive's Page Header Design

Every page (except Meta) starts with a uniform header:

```
HIVE PAGE HEADER (16 bytes)
┌──────────────┬──────────┬────────────┬─────────────┬──────────────┬─────────────┐
│ Page Type    │ Free     │ Slot Count │ Free Space  │ First        │ Checksum    │
│ (u8)         │ Flags(u8)│ (u16)      │ Offset (u16)│ Freeblock(u16)│ (u32)       │
└──────────────┴──────────┴────────────┴─────────────┴──────────────┴─────────────┘
  Byte 0          1         2          4              6               8          12

Followed by: LSN (u32) - Log Sequence Number for WAL recovery  →  Byte 12-16
             Reserved (u32) - Padding for future use            →  Byte 16-20
             
TOTAL HEADER: 20 bytes
```

| Field | Type | Purpose |
|-------|------|---------|
| `page_type` | u8 | One of the page types listed above |
| `free_flags` | u8 | Bit flags: HAS_OVERFLOW, IS_COMPRESSED, etc. |
| `slot_count` | u16 | Number of active slots on this page |
| `free_space_offset` | u16 | Where free space starts (slot array grows up, content grows down from here) |
| `first_freeblock` | u16 | Offset to first freeblock chain (0 = none) |
| `checksum` | u32 | CRC32 of page contents (for corruption detection) |
| `lsn` | u32 | Log Sequence Number — which WAL entry last modified this page |

**Why 20 bytes for the header?**
- Simpler than SQLite's 8/12 byte split. Uniform = less code complexity.
- Includes LSN (Log Sequence Number) for crash recovery. On recovery: if `page LSN >= checkpoint LSN`, replay the WAL frame for this page.
- Includes checksum for corruption detection. A single flipped bit in a 4KB page could cause subtle bugs.

**Design rationale — why not use SQLite's exact header?**
Hive is a graph database, not a relational database. We don't need the "rightmost pointer" in data pages. We DO need an LSN for our WAL recovery model. Keeping it uniform (all page headers same size) is simpler to implement and debug.

### 3.5 Hive's Slotted Page Layout

```
┌──────────────────────────────────────────────────────────────────────────┐
│                      4KB HIVE DATA PAGE                                 │
│                                                                          │
│  ┌─────────────┐  ┌─────────────────┐         ┌─────────────────────┐   │
│  │ Page Header │  │  Slot Directory │  FREE   │   Record Content    │   │
│  │   (20 B)    │  │  (grows  →→→)  │  SPACE  │   Area (←←← grows) │   │
│  └─────────────┘  └─────────────────┘         └─────────────────────┘   │
│  0             19  20             ...                             4095   │
└──────────────────────────────────────────────────────────────────────────┘

SLOT DIRECTORY ENTRY (4 bytes each):
┌────────────────┬──────────────────────┐
│ Record Offset  │ Record Length        │
│ (u16)          │ (u16)                │
└────────────────┴──────────────────────┘

Slot 0 is at byte 20
Slot 1 is at byte 24
Slot N is at byte 20 + (N × 4)
```

**How slots work:**

1. **Insert a record**: 
   - Check `free_space_offset` — is there `record_size + 4` bytes free? (4 for the slot entry)
   - Write record bytes at `free_space_offset - record_size`
   - Decrease `free_space_offset` by `record_size`
   - Add slot entry at `20 + (slot_count × 4)` pointing to the record
   - Increment `slot_count`

2. **Read a record at slot N**:
   - Read slot directory entry at byte `20 + (N × 4)`
   - Get `[offset, length]`
   - Read `length` bytes starting at `offset`

3. **Delete a record at slot N**:
   - Mark slot as free (set offset = 0)
   - Add the freed space to the freeblock chain
   - Optionally: later compact/defragment the page

4. **Update a record at slot N** (grows larger):
   - Old: delete the old record (add to freeblock)
   - New: insert at the end (free space area)
   - Update slot entry offset to point to new location

**Why slot directory instead of fixed offsets?**
- **Variable-width records**: Properties have different numbers of key-value pairs. Labels have different string lengths.
- **No wasted space**: A short record takes only its actual bytes. No padding to 40 bytes.
- **Defragmentation**: When freeblocks accumulate, we can compact all live records to the end and reset the free space offset.

### 3.6 Record Formats Within Pages

With pages, records can be **variable-width** because the slot directory tracks each record's position and size. This is a major improvement over Hive's current fixed-width records.

#### NodeRecord (Variable-Width)

```
NodeRecord (VARIABLE):
┌────────┬──────────┬──────────────┬─────────────────┬─────────────────┬──────────┬────────┐
│ Flags  │ Label ID │ Node ID (u64)│ First Out Edge  │ First In Edge   │ First    │ Props  │
│ (u8)   │ (u32)    │              │ (u64)           │ (u64)           │ Property │ (var)  │
│        │          │              │                 │                 │ (u64)    │        │
└────────┴──────────┴──────────────┴─────────────────┴─────────────────┴──────────┴────────┘
  ←── fixed prefix (37 bytes) ──→                                         ←─ variable ──→
```

- **Fixed prefix** (37 bytes): Always present. Fast access to structural fields without parsing properties.
- **Properties section**: Each property is `[key_hash: u64][value_type: u8][value_bytes: var]`. Inline for small values, pointer to string store for large ones. This is similar to SQLite's payload encoding but adapted for key-value pairs.

**Why keep the node ID in the record when the record position implies the ID?**
With pages, node ID no longer maps to a byte offset (offset = `node_id * 40`). Records are packed in pages. We still store the node ID in the record because:
1. Slots can be reordered (compaction).
2. Records from different pages are different by definition.
3. It acts as a self-check: read record, verify its ID.

#### EdgeRecord (Variable-Width)

```
EdgeRecord (VARIABLE):
┌────────┬──────────┬──────────┬──────────┬──────────────┬──────────────┬──────────┬────────┐
│ Flags  │ Label ID │ Edge ID  │ Src Node │ Dst Node     │ Next Out     │ Next In  │ First  │ Props │
│ (u8)   │ (u32)    │ (u64)    │ (u64)    │ (u64)        │ Edge (u64)   │ Edge(u64)│ Prop   │ (var) │
│        │          │          │          │              │              │          │ (u64)   │       │
└────────┴──────────┴──────────┴──────────┴──────────────┴──────────────┴──────────┴────────┴───────┘
  ←────────────────── fixed prefix (61 bytes) ──────────────────────→                     ←─var──→
```

Similar to NodeRecord with fixed prefix + variable properties.

#### PropertyRecord (Variable-Width)

```
PropertyRecord (VARIABLE):
┌────────┬──────────┬──────────────┬────────────┬───────────┬──────────────┬──────────┬──────────┐
│ Flags  │ Prop ID  │ Key Hash     │ Key Offset │ Value     │ Value        │ Next     │ Reserved │
│ (u8)   │ (u64)    │ (u64)        │ (u64)      │ Type (u8) │ Blob (var)   │ Prop(u64)│ (u32)    │
└────────┴──────────┴──────────────┴────────────┴───────────┴──────────────┴──────────┴──────────┘
```

The value blob is the most variable part — booleans (1 byte), integers (1-8 bytes), floats (8 bytes), short strings (up to ~20 bytes inline), long strings (pointer to string store).

### 3.7 Why This Design? Design Rationale

| Design Choice | Why |
|---------------|-----|
| **4KB page size** | OS page size on most systems. Aligns with filesystem blocks. Big enough for ~100 small records, small enough for efficient caching. |
| **Slotted pages** | Supports variable-width records. Standard in PostgreSQL, SQLite, MySQL/InnoDB. Battle-tested. |
| **Separate page types per entity** | Nodes, edges, and properties have different layouts and access patterns. Separate page types simplify code and optimize cache usage. |
| **20-byte uniform header** | Simpler than SQLite's dual header sizes. One code path for all page types. |
| **LSN in page header** | Enables page-level WAL recovery: check LSN against checkpoint, replay if needed. |
| **Checksum in page header** | Catches bit-rot and I/O corruption early. Cheap insurance (CRC32). |
| **Variable-width records** | Properties can have any number of key-value pairs. A "Person" node with 2 properties should NOT take the same space as a "Company" with 20. |
| **Slot directory with (offset, length)** | O(1) access to any record by slot number. Defragmentation possible without breaking references. |
| **Freeblock chain** | Space from deleted records is tracked without immediate compaction. Compaction happens lazily when free space is critically low. |

**What we DON'T need (that SQLite has):**
- **Rightmost pointer in data pages**: SQLite uses this for B-tree interior nodes. Hive's data pages (node/edge/property) are NOT B-tree nodes — they're heap-organized. The B-tree sits on top as a separate index structure.
- **Cell pointer array with only offset (no length)**: SQLite stores cell sizes in the cell itself. We store `(offset, length)` in the slot — more explicit, easier to debug.

---

## 4. Architecture & System Diagrams

### 4.1 High-Level System Architecture

```
┌───────────────────────────────────────────────────────────────────────────────────┐
│                              HIVE DB ENGINE                                       │
│                                                                                   │
│  ┌─────────────────────────────────────────────────────────────────────────────┐  │
│  │                           QUERY LAYER                                       │  │
│  │  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────────────────┐   │  │
│  │  │ Parser   │───▶│ Planner  │───▶│ Executor │───▶│ B-Tree Cursor        │   │  │
│  │  │ (pest)   │    │          │    │          │    │ (page-aware traversal)│   │  │
│  │  └──────────┘    └──────────┘    └──────────┘    └──────────┬───────────┘   │  │
│  └──────────────────────────────────────────────────────────────┼──────────────┘  │
│                                                                 │                  │
│  ┌──────────────────────────────────────────────────────────────┼──────────────┐  │
│  │                           STORAGE LAYER                      │              │  │
│  │                                                              ▼              │  │
│  │  ┌──────────────────────────────────────────────────────────────────────┐   │  │
│  │  │                          PAGER                                       │   │  │
│  │  │  • read_page(pgno) → PageRef                                         │   │  │
│  │  │  • allocate_page() → PageRef                                         │   │  │
│  │  │  • free_page(pgno)                                                   │   │  │
│  │  │  • add_dirty(page)   • commit()   • checkpoint()                     │   │  │
│  │  └──────┬───────────────┬────────────────────────┬──────────────────────┘   │  │
│  │         │               │                        │                          │  │
│  │         ▼               ▼                        ▼                          │  │
│  │  ┌──────────┐   ┌──────────────┐         ┌──────────────┐                   │  │
│  │  │ Page     │   │ Buffer Pool  │         │ Page Cache   │                   │  │
│  │  │ Format   │   │ (Arena)      │         │ (SIEVE)      │                   │  │
│  │  │ Layout   │   │              │         │              │                   │  │
│  │  └──────────┘   └──────────────┘         └──────────────┘                   │  │
│  │         │                                                                   │  │
│  └─────────┼───────────────────────────────────────────────────────────────────┘  │
│            │                                                                      │
│  ┌─────────┼───────────────────────────────────────────────────────────────────┐  │
│  │         ▼                    I/O LAYER                                      │  │
│  │  ┌──────────────┐         ┌──────────────┐                                  │  │
│  │  │ WAL File     │         │ Main DB File │                                  │  │
│  │  │ (physical)   │         │ (pages)      │                                  │  │
│  │  └──────────────┘         └──────────────┘                                  │  │
│  └─────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                   │
└───────────────────────────────────────────────────────────────────────────────────┘
```

### 4.2 Component Interaction Diagram

```
                        ┌───────────────────────────────────────┐
                        │              HiveDb                    │
                        │  (public API: create_node, get_edge,  │
                        │   execute_query, begin_transaction)   │
                        └──────────────────┬────────────────────┘
                                           │
                    ┌──────────────────────┼──────────────────────┐
                    │                      │                      │
                    ▼                      ▼                      ▼
          ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐
          │   Query Layer    │  │  Index Layer     │  │  Transaction Mgr │
          │  (parser,plan,   │  │  (BTree indexes  │  │  (wal, savepoint, │
          │   executor)      │  │   on pages)      │  │   commit/rollback)│
          └────────┬─────────┘  └────────┬─────────┘  └────────┬─────────┘
                   │                     │                      │
                   └─────────────────────┼──────────────────────┘
                                         │
                                         ▼
                              ┌──────────────────────┐
                              │       Pager          │
                              │  ┌────────────────┐  │
                              │  │  Page Cache    │  │  ← SIEVE eviction
                              │  │  (HashMap +    │  │
                              │  │   linked list) │  │
                              │  └───────┬────────┘  │
                              │          │            │
                              │  ┌───────▼────────┐  │
                              │  │  Buffer Pool   │  │  ← Arena allocator
                              │  │  (Vec of 4KB   │  │
                              │  │   buffers)     │  │
                              │  └───────┬────────┘  │
                              └──────────┼───────────┘
                                         │
                              ┌──────────┼───────────┐
                              │          │           │
                              ▼          ▼           ▼
                        ┌─────────┐ ┌─────────┐ ┌─────────┐
                        │ WAL File│ │ Main DB │ │ Temp    │
                        │ (*.wal) │ │ (*.hive)│ │ Files   │
                        └─────────┘ └─────────┘ └─────────┘
```

### 4.3 Page Read Flow (Detailed)

```
Caller: "Give me node with ID 5000"

Step 1: TRANSLATE ID TO PAGE
┌──────────────────────────────────────────────────────┐
│ node_id = 5000                                       │
│ records_per_page ≈ (4096 - 20) / 40 ≈ 101            │
│ page_no = 5000 / 101 + 1 = 50                        │
│ slot_within_page = 5000 % 101 = 50                   │
└──────────────────────────────────────────────────────┘
                        │
                        ▼
Step 2: PAGER - CHECK CACHE
┌──────────────────────────────────────────────────────┐
│ page_cache.get(PageCacheKey(50))                     │
│                                                      │
│ HIT  ──▶ return cached PageRef                       │
│ MISS ──▶ continue                                    │
└──────────────────────────────────────────────────────┘
                        │
                        ▼
Step 3: PAGER - READ FROM DISK
┌──────────────────────────────────────────────────────┐
│ 1. Check WAL: is there a newer version at page 50?   │
│ 2. If yes: read from WAL                             │
│ 3. If no:  read from main DB file                    │
│     seek(50 * 4096), read(4096 bytes)                │
│ 4. Get buffer from Buffer Pool                       │
│ 5. Copy bytes into buffer                            │
│ 6. Verify checksum (CRC32)                           │
│ 7. Mark PAGE_LOADED                                  │
│ 8. Insert into page_cache                            │
│ 9. Return PageRef                                    │
└──────────────────────────────────────────────────────┘
                        │
                        ▼
Step 4: READ RECORD FROM PAGE
┌──────────────────────────────────────────────────────┐
│ page = PageRef for page 50                           │
│                                                      │
│ header = read_page_header(&page.buffer[0..20])        │
│ slot_count = header.slot_count                       │
│                                                      │
│ slot_entry = read_slot(&page.buffer, 50)             │
│ // slot_entry = { offset: 3500, length: 42 }         │
│                                                      │
│ record_bytes = &page.buffer[3500..3542]              │
│ node = NodeRecord::from_bytes(record_bytes)           │
│                                                      │
│ // Verify: node.id == 5000                           │
│ // Pin page (increment pin_count)                    │
│ return Node { ... }                                  │
└──────────────────────────────────────────────────────┘
```

### 4.4 Page Write Flow (Detailed)

```
Caller: "Create a new node with label 'Person', properties {name: 'Alice'}"

Step 1: FIND OR ALLOCATE A PAGE WITH SPACE
┌──────────────────────────────────────────────────────┐
│ // Check last DataNode page                          │
│ last_page = pager.read_page(latest_data_node_page)   │
│ free_space = PAGE_SIZE - header.free_space_offset     │
│            - (slot_count + 1) * SLOT_ENTRY_SIZE       │
│                                                      │
│ if free_space >= new_record_size:                    │
│     target_page = last_page                          │
│ else:                                                │
│     target_page = pager.allocate_page(DataNode)      │
│     init_page_header(target_page, DataNode)           │
│     pager.add_dirty(target_page)                     │
└──────────────────────────────────────────────────────┘
                        │
                        ▼
Step 2: WRITE RECORD INTO PAGE
┌──────────────────────────────────────────────────────┐
│ page = target_page                                   │
│ mut buf = page.buffer.as_mut_slice()                 │
│                                                      │
│ header = read_page_header(buf)                       │
│ new_offset = header.free_space_offset - record_len   │
│                                                      │
│ // Write record bytes at new_offset                  │
│ buf[new_offset..new_offset+record_len] = record_bytes│
│                                                      │
│ // Write slot entry                                  │
│ slot_pos = 20 + header.slot_count * 4                │
│ buf[slot_pos..slot_pos+2] = new_offset.to_be_bytes() │
│ buf[slot_pos+2..slot_pos+4] = record_len.to_be_bytes()│
│                                                      │
│ // Update header                                     │
│ header.slot_count += 1                               │
│ header.free_space_offset = new_offset                │
│ header.checksum = crc32(&buf[12..4096])             │
│ write_page_header(buf, header)                       │
│                                                      │
│ pager.add_dirty(page)                                │
└──────────────────────────────────────────────────────┘
                        │
                        ▼
Step 3: COMMIT (WRITE TO WAL)
┌──────────────────────────────────────────────────────┐
│ pager.commit()                                       │
│                                                      │
│ for each dirty_page in dirty_pages:                  │
│     // Build WAL frame                                │
│     frame = [                                        │
│         page_no: u32,                                │
│         db_size: u32,                                │
│         salt1: u32, salt2: u32,                      │
│         checksum1: u32, checksum2: u32,              │
│         page_data: page.buffer.clone()  // 4KB       │
│     ]                                                │
│     wal.append(&frame)                               │
│                                                      │
│ wal.fsync()  // DURABILITY POINT                     │
│ mark all dirty pages as SPILLED                      │
└──────────────────────────────────────────────────────┘
```

### 4.5 WAL and Recovery Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         WAL LIFECYCLE                                       │
│                                                                             │
│  [Append Phase]         [Commit Phase]         [Checkpoint Phase]           │
│                                                                             │
│  Page modifications  →  WAL Frames written  →  Frames copied to main DB     │
│  accumulate in RAM      to wal.hive            file, then WAL truncated     │
│                                                                             │
│  ┌──────┐              ┌──────────┐           ┌──────────────┐              │
│  │ DIRTY │──────────▶  │ WAL ENTRY│──────────▶│ DB FILE PAGE │              │
│  │ PAGE  │             │ (frame)  │           │ (permanent)  │              │
│  └──────┘              └──────────┘           └──────────────┘              │
│                                                                             │
│  ═══════════════════════════════════════════════════════════════════════    │
│                         RECOVERY ON STARTUP                                 │
│                                                                             │
│  1. Open DB file                                                            │
│  2. Detect WAL file exists                                                  │
│  3. Read WAL header → last_checkpoint_lsn                                   │
│  4. For each frame in WAL:                                                  │
│     if frame.lsn > last_checkpoint_lsn:                                     │
│         // This frame was committed but not checkpointed                    │
│         // Either: (a) copy it to DB file, or (b) keep in WAL for           │
│         // next checkpoint (WAL mode)                                       │
│         copy_frame_to_db(frame)                                             │
│  5. fsync(DB file)                                                          │
│  6. Write checkpoint record to WAL                                          │
│  7. Truncate WAL                                                            │
│  8. DB is now consistent                                                    │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Hive's current WAL is logical** (`WalEntry::CreateNode`, etc.). With pages, we switch to **physical WAL** (page images). Here's why:

| Aspect | Logical WAL (current) | Physical WAL (with pages) |
|--------|----------------------|---------------------------|
| Recovery speed | Must replay every operation sequentially | Can copy pages in any order, or just leave in WAL |
| Atomicity | Complex — need to ensure operation boundaries | Simple — a full page is either written or not |
| Space | Small entries | Larger (full 4KB pages) but WAL truncation keeps it bounded |
| Crash at any point | Need to know which entries are "done" | Frame-level checksums detect partial writes |
| Interaction with buffer pool | None (separate systems) | Natural — dirty pages = WAL frames |

**The plan for Hive**: During the transition, support both. Old logical entries for backward compatibility on existing DBs. New physical frames for page-based DBs.

---

## 5. Production Implementation Plan

Here's how to build this incrementally, testing at each step. The plan follows the dependency order: you can't build the page cache before you have pages, and you can't build B-trees before the page cache.

### 5.1 Phase 1: Page Abstraction & On-Disk Format

**Files to create:**
```
core/storage/page/
├── mod.rs              # Module declarations
├── format.rs           # Page header, slot directory, page types
├── layout.rs           # Functions: init_page, insert_record, read_record, delete_record
├── record.rs           # Variable-width NodeRecord, EdgeRecord, PropertyRecord
└── serialize.rs        # Varint encoding/decoding, binary read/write helpers
```

**What to build:**
1. `PageHeader` struct (20 bytes as designed above)
2. `PageType` enum with all 10 types
3. `SlotEntry` struct: `{ offset: u16, length: u16 }`
4. Functions operating on `&mut [u8; 4096]`:
   - `init_page(buf, page_type)` — write fresh header
   - `insert_record(buf, record_bytes) → SlotIndex` — append record, update header
   - `read_record(buf, slot_idx) → &[u8]` — get record by slot
   - `delete_record(buf, slot_idx)` — mark slot free, add to freeblock
   - `update_record(buf, slot_idx, new_bytes)` — delete old + insert new
   - `compact_page(buf)` — defragment freeblocks, pack live records
5. Varint encoder/decoder (copy from Turso or simplify)
6. Variable-width `NodeRecordV2`, `EdgeRecordV2`, `PropertyRecordV2` with `to_bytes()` / `from_bytes()`

**Unit tests:**
- Init a page, verify header fields
- Insert 100 records, verify all can be read back
- Delete records, verify freeblock chain
- Update records, verify old space freed, new space used
- Compact after deletes, verify all records still readable

### 5.2 Phase 2: Buffer Pool

**File to create:** `core/storage/buffer_pool.rs`

**What to build:**
```rust
pub struct BufferPool {
    buffers: Vec<Box<[u8; PAGE_SIZE]>>,     // Pre-allocated page buffers
    free_list: VecDeque<usize>,             // Indices of free buffers
}

impl BufferPool {
    pub fn new(pool_size: usize) -> Self;
    pub fn acquire(&mut self) -> Option<Box<[u8; PAGE_SIZE]>>;
    pub fn release(&mut self, buffer: Box<[u8; PAGE_SIZE]>);
    pub fn available(&self) -> usize;
}
```

**Why pre-allocate?** Avoids `malloc` per page read. The pool starts with e.g. 2000 buffers (8MB for 4KB pages) and reuses them.

**Configuration:** Default pool size = 2000 pages = 8MB. Configurable via `HiveDb::open_with_config()`.

### 5.3 Phase 3: Page Cache (SIEVE)

**File to create:** `core/storage/page_cache.rs`

**What to build:**
1. `PageCacheKey(usize)` — wraps page number
2. `PageCacheEntry` — holds `PageRef`, `ref_bit: u8`
3. Intrusive linked list (use `std::collections::LinkedList` or write a simple one)
4. `HashMap<PageCacheKey, *mut PageCacheEntry>` for O(1) lookup
5. `clock_hand` pointer for SIEVE eviction
6. Methods: `get()`, `insert()`, `evict_one()`, `clear()`

**Why intrusive linked list?**
Turso uses `intrusive_collections` crate to store the linked list links inside the entry itself (not in a separate node). This avoids a second allocation per entry. For Hive, we can start with `std::collections::LinkedList` for simplicity and optimize later if profiling shows it's a bottleneck.

### 5.4 Phase 4: Pager & WAL Integration

**File to create:** `core/storage/pager.rs`

**What to build:**
```rust
pub struct Pager {
    db_file: File,                          // Main database file
    wal_file: File,                         // Write-ahead log
    header: DbHeader,                       // In-memory copy of database header
    page_cache: PageCache,                  // SIEVE cache
    buffer_pool: BufferPool,                // Memory arena
    dirty_pages: RoaringBitmap,             // Set of dirty page IDs
    wal_header: WalHeader,                  // WAL state
}

impl Pager {
    pub fn open(db_path: &Path) -> Result<Self>;
    pub fn read_page(&self, pgno: usize) -> Result<PageRef>;
    pub fn allocate_page(&self, page_type: PageType) -> Result<PageRef>;
    pub fn free_page(&self, pgno: usize) -> Result<()>;
    pub fn add_dirty(&self, page: &PageRef);
    pub fn commit(&self) -> Result<()>;
    pub fn checkpoint(&self) -> Result<()>;
}
```

**Key integration points:**
- `read_page()`: Check cache → Check WAL → Read from DB file → Insert in cache
- `allocate_page()`: Check freelist → Extend file → Init page → Mark dirty
- `commit()`: Scan dirty pages → Write WAL frames → Fsync WAL → Mark spilled
- `checkpoint()`: Copy WAL pages to DB file → Fsync DB → Update WAL header → Truncate WAL

**WAL format** (physical, per-page):
```
Frame = [page_number: u32][page_data: [u8; 4096]][checksum: u32]
```

### 5.5 Phase 5: B-Tree Indexes on Pages

Once pages, cache, and pager are working, B-tree indexes sit naturally on top.

**File to create:** `core/storage/btree.rs`

**What to build:**
1. `BTreeCursor` — navigates interior/leaf pages
2. `btree_insert(page, key, value)` — insert into leaf, split if full, propagate up
3. `btree_delete(page, key)` — delete from leaf, merge if underfull
4. `btree_search(key) → value` — traverse interior→leaf, return value
5. Balance algorithm (page split/merge)

**Why build your own instead of using a crate?**
- B-tree nodes ARE pages. You need tight coupling with your pager and cache.
- Standard crates (`std::collections::BTreeMap`) are in-memory only.
- You control the on-disk format — important for cross-version compatibility.

---

## 6. Key File References

### Turso/Limbo (Reference Implementation)

| File | Lines | What It Does |
|------|-------|--------------|
| `core/storage/sqlite3_ondisk.rs` | 2444 | On-disk format: page types, cells, varints, WAL frames, checksums |
| `core/storage/pager.rs` | 6299 | Page lifecycle: read, allocate, free, dirty, commit, checkpoint |
| `core/storage/page_cache.rs` | 1832 | SIEVE eviction cache with spill management |
| `core/storage/btree.rs` | 12817 | B-tree cursor, balance, cell insertion/deletion on pages |
| `core/storage/buffer_pool.rs` | 1769 | Arena-based memory allocator for page buffers |
| `core/storage/encryption.rs` | ~75 | Page-level AES-GCM/AEGIS encryption |
| `core/storage/checksum.rs` | — | Page checksum computation |

### Hive DB (Current Implementation)

| File | Lines | What It Does |
|------|-------|--------------|
| `core/store/node/store.rs` | 110 | Fixed-width node store — to be replaced by pages |
| `core/store/node/record.rs` | 59 | 40-byte fixed NodeRecord — to become variable-width |
| `core/store/edge/store.rs` | ~110 | Fixed-width edge store — to be replaced |
| `core/store/edge/record.rs` | ~59 | 56-byte fixed EdgeRecord — to become variable-width |
| `core/store/property/store.rs` | ~110 | Fixed-width property store — to be replaced |
| `core/store/property/record.rs` | ~59 | 56-byte fixed PropertyRecord — to become variable-width |
| `core/db/hive_db.rs` | 1238 | Main DB handle — will use Pager instead of individual stores |
| `core/wal/wal.rs` | 128 | Logical WAL — to become physical page-level WAL |
| `core/wal/wal_entry.rs` | 183 | Logical WAL entries — to be replaced by page frames |
| `core/store/header.rs` | ~60 | 52-byte DB header — to become Meta page (page 1) |
| `docs/Study.md` | 117 | Roadmap — exactly what we're implementing now |

---

## Summary

**The central insight**: Pages are not just a file format. They are the foundation on which EVERY production database feature is built — caching, concurrency, crash recovery, indexing, and query optimization.

Turso/Limbo shows us a battle-tested production implementation that we can learn from. The key patterns to adopt:

1. **Slotted page layout** — variable-width records, dual-growing regions, slot directory
2. **SIEVE cache** — efficient multi-bit eviction that outperforms simple LRU
3. **Arena buffer pool** — pre-allocated memory to avoid allocation overhead
4. **Physical WAL** — page images instead of logical operations for simpler, faster recovery
5. **B-trees on pages** — indexes that scale to disk sizes

Hive's migration from flat files to pages is the single most important step toward a production-grade graph database. Everything else — better query execution, concurrency, compaction — builds on this foundation.
