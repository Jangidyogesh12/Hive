# Storage Format

Hive stores each database as a directory of binary files. The files are local to the process and are opened directly by the Rust engine. (Contributor note: the end-to-end story — transactions, WAL, indexes — is in `HIVE.md` at the repository root.)

## Files

| File | Purpose |
|---|---|
| `hive.db` | Paged store (4 KiB pages): meta page, slotted data/dictionary/B-tree/overflow/freelist pages |
| `wal.hive` | Write-ahead log (length-delimited page-image entries with checksums, checkpoint/truncate on reopen) |

## Pages

Every page is `PAGE_SIZE` (4096) bytes. Page 0 is the meta page (100-byte `MetaHeader`); all other pages start with a 20-byte `PageHeader`:

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | Page type tag |
| 1 | 1 | Feature flags (bit 0 = has overflow, bit 1 = compressed) |
| 2 | 2 | Slot count |
| 4 | 2 | Free-space offset (content area grows downward from end of page) |
| 6 | 2 | First freeblock offset (page-local free chain), or 0 |
| 8 | 4 | CRC32 checksum (computed from offset 12 onward) |
| 12 | 4 | Highest LSN written to this page |
| 16 | 4 | Reserved |

Page types (`core/storage/page/format.rs`):

| Tag | Type | Holds |
|---|---|---|
| `0x00` | `Meta` | Database header (page 0 only) |
| `0x01` | `DataNode` | Node records |
| `0x02` | `DataEdge` | Edge records |
| `0x04` | `StringData` | Legacy string data (unused, format compatibility) |
| `0x05` | `LabelData` | Label/type dictionary entries (`label_id -> name`) |
| `0x06` | `PropertyKeyData` | Property-key dictionary entries (`key_id -> name`) |
| `0x0A` | `IndexInterior` | B+tree interior pages |
| `0x0B` | `IndexLeaf` | B+tree leaf pages |
| `0x0F` | `Freelist` | Free-page list (`[next_page][count][page_id …]`) |
| `0x10` | `Overflow` | Long-string bytes |

The meta page stores magic bytes, format version (`CURRENT_VERSION = 3`), page size, allocation counters (`node_count`, `edge_count` — never decremented on delete — plus `property_count`, `label_count`), root page pointers (node, edge, label, property-key, index catalog), `freelist_head`, `schema_version`, checksum, and LSN. `HiveDb::open` rejects files with unexpected magic or unsupported versions.

## Slotted Pages

Data, dictionary, and B-tree pages share the slotted layout:

- The slot table starts at byte 20 and grows downward (4 bytes per slot: content offset + record length; a dead marker denotes deleted records).
- Record content starts at the end of the page and grows upward.
- `insert_record` appends a slot; `insert_record_at` reuses a dead slot; `update_record` rewrites in place when the new bytes fit, otherwise deletes and re-inserts at the same slot.
- `compact_page` repacks live records contiguously **without renumbering slots**: dead slots stay dead and live slots keep their indices, so packed record IDs `(page_id, slot_id)` remain valid. The freeblock chain is dropped because free space becomes contiguous again.

## Records

Node, edge, and property entries are variable-width and addressed by packed record ID. IDs map to `(page_id, slot_id)`, not to file positions.

Nodes store (fixed prefix):

- Flags, primary `label_id` (0 = unlabeled), logical node ID
- First outgoing edge ID, first incoming edge ID (packed IDs, `NIL_ID` when none)
- Reserved `first_property` field, property-entry count
- Inline property entries, then `[extra_label_count: u16][extra_label_id: u32 …]` for `(n:A:B)` multi-label nodes

Edges store (fixed prefix):

- Flags, type `label_id`, logical edge ID, source and destination node IDs (packed)
- `next_out_edge` / `next_in_edge` chain links, reserved `first_property` field
- Property-entry count followed by inline property entries

Property entries store:

- `key_id` into the property-key dictionary (collision-safe identity; no hash-only lookup)
- Value type tag + 15-byte inline buffer, plus an overflow offset for long strings

## Property Values

Values are represented by a type tag and a 15-byte inline buffer.

Supported value types:

- Null
- Integer (`i64`)
- Float (`f64`)
- Boolean
- Short string (fits in the 15-byte buffer)
- Long string (stored in an `Overflow` page; the entry holds the page offset)

`Map` and `List` values exist only as query-result values (whole-entity returns) and are never stored inline.

## Dictionaries And Labels

Labels and property keys are dictionary-encoded to compact numeric IDs:

- `LabelData` pages map `label_id -> name`; shared by node labels and edge types. Registration is transactional: failed queries roll new labels back, committed labels survive reopen and WAL recovery.
- `PropertyKeyData` pages map `key_id -> name`. Duplicate registration returns the existing ID. Property writes register keys inside the same transaction as the record update.

## Adjacency

Hive keeps adjacency through linked edge chains instead of separate adjacency lists.

- Each node points to its first outgoing and first incoming edge.
- Each edge points to the next edge in the source node's outgoing chain and the next edge in the destination node's incoming chain.
- Edge creation prepends to both chains; edge deletion unlinks from both chains. All chain updates participate in the enclosing transaction.
- One-hop traversal walks the chains; variable-length traversal BFS-expands them with a visited set and an 8-hop guardrail.

This makes one-hop traversal local to node and edge records while keeping storage append-friendly.

## Indexes

Indexes live inside `hive.db` as B+tree pages, coordinated by an index catalog (itself a B+tree keyed by `(entity_kind, label_id, property_key_id)` → index root page):

- Node label lookups, edge type lookups
- Node/edge property lookups, per-label (or per-type) and global
- Unique constraints on `(label, property_key)`, backed by a per-label property index

B+tree leaf cells store `[key_len][key][rid_count][rid …]`; interior cells store `[key_len][key][right_child_page]` with the leftmost pointer in the page header. Keys use a canonical byte encoding (signed integers via sign-bit flip) so byte order matches logical order. Splits, root growth, inserts, and deletes are WAL-protected through transaction before-images. Indexes can be verified with `check_index_consistency`.

## Space Reuse

- Deleted record slots are marked dead; pages track them in a page-local freeblock chain and the database tracks free pages in persistent `Freelist` pages chained from `freelist_head`.
- The freelist is written out on sync and reloaded on open, so freed pages survive restart; rolled-back allocations return to the freelist.
- `find_or_alloc_page` prefers the type's root page when it has room, reuses freelist pages, and otherwise allocates at end of file.

## WAL And Recovery

Mutation commits append page-image entries to `wal.hive` before the changes are considered durable; each entry is length-delimited with type tags and checksums. Commits auto-checkpoint roughly every 64 transactions. On open, Hive replays committed entries after the last checkpoint (page-image redo with LSN stamping), reports the replay counts, checkpoints, and truncates clean WAL state. Uncommitted work is never replayed. Read-only query plans skip WAL work entirely (`commit_readonly`).

Page LSNs are stamped per page layout: regular pages carry the LSN in the page header, while page 0 carries it in the meta header (stamping page 0 as a regular page would destroy the magic bytes). Recovery extracts each page's LSN with the matching layout, or redo silently skips pages.

## Read-Only Snapshots

`HiveDb::open_snapshot` opens a fully private, read-only view of a database directory for concurrent readers: own file handles, own cache, WAL redo applied into that cache only — the shared files are never written (dirty evictions are dropped, allocation and checkpoints are rejected). Unknown labels/property keys resolve to a transient ID that matches nothing instead of being registered. Each snapshot observes committed state as of its own open; writers are excluded for the snapshot's lifetime by the `SharedDb` admission lock.

## Compatibility

The header stores magic bytes and a version number. `HiveDb::open` rejects unsupported versions to avoid silently interpreting incompatible files. The current format version is 3; database files written before multi-label support (the trailing extra-label record section) should be recreated — no on-disk migration is implemented.
