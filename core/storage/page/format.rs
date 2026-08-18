/// On-disk page format definitions: page types, page header, meta header, and constants.
use super::serializer;

pub const PAGE_SIZE: usize = 4096;
pub const REGULAR_HEADER_SIZE: usize = 20;
pub const META_HEADER_SIZE: usize = 100;
pub const SLOT_ENTRY_SIZE: usize = 4;
pub const HIVE_MAGIC: [u8; 16] = [b'H', b'I', b'V', b'E', 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
pub const CURRENT_VERSION: u32 = 3;
pub const META_PAGE_ID: u32 = 0;

/// Returns true if the page buffer starts with the Hive magic bytes.
pub fn is_meta_page(buf: &[u8; PAGE_SIZE]) -> bool {
    buf[..16] == HIVE_MAGIC
}

/// Database page type tag stored in the first byte of every page header.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageType {
    /// Database meta header (page 0 only).
    Meta = 0x00,
    /// Page containing node records.
    DataNode = 0x01,
    /// Page containing edge records.
    DataEdge = 0x02,
    /// Legacy string data page (unused, kept for format compatibility).
    StringData = 0x04,
    /// Page containing label dictionary entries.
    LabelData = 0x05,
    /// Page containing property-key dictionary entries.
    PropertyKeyData = 0x06,
    /// B-tree interior index page (reserved for future use).
    IndexInterior = 0x0A,
    /// B-tree leaf index page (reserved for future use).
    IndexLeaf = 0x0B,
    /// Free page list tracking reusable pages.
    Freelist = 0x0F,
    /// Overflow page for long string storage.
    Overflow = 0x10,
}

impl PageType {
    /// Converts the on-disk page type byte into a known page type.
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Self::Meta),
            0x01 => Some(Self::DataNode),
            0x02 => Some(Self::DataEdge),
            0x04 => Some(Self::StringData),
            0x05 => Some(Self::LabelData),
            0x06 => Some(Self::PropertyKeyData),
            0x0A => Some(Self::IndexInterior),
            0x0B => Some(Self::IndexLeaf),
            0x0F => Some(Self::Freelist),
            0x10 => Some(Self::Overflow),
            _ => None,
        }
    }
}

/// On-disk page header stored at the beginning of every non-meta page.
///
/// Followed by a slot table growing downward and a record area growing upward.
pub struct PageHeader {
    /// Page type tag (see `PageType`).
    pub page_type: PageType,
    /// Feature flags: bit 0 = has overflow, bit 1 = compressed.
    pub free_flags: u8,
    /// Number of slot entries currently in the slot table.
    pub slot_count: u16,
    /// Byte offset where the next record can be written (grows downward).
    pub free_space_offset: u16,
    /// Byte offset of the first entry in the freeblock chain, or 0 if none.
    pub first_freeblock: u16,
    /// CRC32 checksum of the page bytes starting at `CHECKSUM_START`.
    pub checksum: u32,
    /// Highest LSN written to this page (used by WAL recovery).
    pub lsn: u32,
    /// Reserved field for future use.
    pub reserved: u32,
}

impl PageHeader {
    pub const HAS_OVERFLOW: u8 = 0x01;
    pub const IS_COMPRESSED: u8 = 0x02;

    pub const CHECKSUM_START: usize = 12;

    /// Creates a new empty regular-page header for the given page kind.
    pub fn new(page_type: PageType) -> Self {
        Self {
            page_type,
            free_flags: 0,
            slot_count: 0,
            free_space_offset: PAGE_SIZE as u16,
            first_freeblock: 0,
            checksum: 0,
            lsn: 0,
            reserved: 0,
        }
    }

    /// Decodes a regular-page header from the beginning of a page buffer.
    pub fn from_bytes(buf: &[u8]) -> Self {
        Self {
            page_type: PageType::from_u8(serializer::get_u8(buf, 0)).unwrap_or(PageType::DataNode),
            free_flags: serializer::get_u8(buf, 1),
            slot_count: serializer::get_u16_le(buf, 2),
            free_space_offset: serializer::get_u16_le(buf, 4),
            first_freeblock: serializer::get_u16_le(buf, 6),
            checksum: serializer::get_u32_le(buf, 8),
            lsn: serializer::get_u32_le(buf, 12),
            reserved: serializer::get_u32_le(buf, 16),
        }
    }

    /// Encodes this regular-page header into the beginning of a page buffer.
    pub fn to_bytes(&self, buf: &mut [u8]) {
        buf[0..REGULAR_HEADER_SIZE].fill(0);
        serializer::put_u8(buf, 0, self.page_type as u8);
        serializer::put_u8(buf, 1, self.free_flags);
        serializer::put_u16_le(buf, 2, self.slot_count);
        serializer::put_u16_le(buf, 4, self.free_space_offset);
        serializer::put_u16_le(buf, 6, self.first_freeblock);
        serializer::put_u32_le(buf, 8, self.checksum);
        serializer::put_u32_le(buf, 12, self.lsn);
        serializer::put_u32_le(buf, 16, self.reserved);
    }
}

/// Database-wide metadata header stored on page 0.
///
/// Contains magic bytes, version, page size, record counters, root page
/// pointers, and WAL recovery state.  All fields are little-endian.
pub struct MetaHeader {
    /// Magic bytes identifying a Hive database file.
    pub magic: [u8; 16],
    /// On-disk format version.
    pub version: u32,
    /// Page size in bytes (always 4096).
    pub page_size: u32,
    /// Total number of pages in the database file.
    pub db_size_pages: u32,
    /// Monotonically increasing counter of created nodes (allocation counter, never decremented).
    pub node_count: u64,
    /// Monotonically increasing counter of created edges (allocation counter, never decremented).
    pub edge_count: u64,
    /// Monotonically increasing counter of property keys.
    pub property_count: u64,
    /// Monotonically increasing counter of labels.
    pub label_count: u64,
    /// Page ID of the root node page, or 0 if none.
    pub root_node_page: u32,
    /// Page ID of the root edge page, or 0 if none.
    pub root_edge_page: u32,
    /// Page ID of the root label dictionary page, or 0 if none.
    pub root_label_page: u32,
    /// Page ID of the root property-key dictionary page, or 0 if none.
    pub root_string_page: u32,
    /// Page ID of the freelist head, or 0 if none.
    pub freelist_head: u32,
    /// Page ID of the root B-tree index page, or 0 if none.
    pub root_index_page: u32,
    /// User-facing schema version number.
    pub schema_version: u32,
    /// CRC32 checksum of the meta page bytes.
    pub checksum: u32,
    /// Highest LSN written to this page (used by WAL recovery).
    pub lsn: u32,
}

impl MetaHeader {
    /// Creates the initial database metadata header for a new Hive database.
    pub fn new() -> Self {
        Self {
            magic: HIVE_MAGIC,
            version: CURRENT_VERSION,
            page_size: PAGE_SIZE as u32,
            db_size_pages: 1,
            node_count: 0,
            edge_count: 0,
            property_count: 0,
            label_count: 0,
            root_node_page: 0,
            root_edge_page: 0,
            root_label_page: 0,
            root_string_page: 0,
            freelist_head: 0,
            root_index_page: 0,
            schema_version: 0,
            checksum: 0,
            lsn: 0,
        }
    }

    /// Decodes the database metadata header from page 0 bytes.
    pub fn from_bytes(buf: &[u8]) -> Self {
        let mut magic = [0u8; 16];
        magic.copy_from_slice(&buf[0..16]);
        Self {
            magic,
            version: serializer::get_u32_le(buf, 16),
            page_size: serializer::get_u32_le(buf, 20),
            db_size_pages: serializer::get_u32_le(buf, 24),
            node_count: serializer::get_u64_le(buf, 28),
            edge_count: serializer::get_u64_le(buf, 36),
            property_count: serializer::get_u64_le(buf, 44),
            label_count: serializer::get_u64_le(buf, 52),
            root_node_page: serializer::get_u32_le(buf, 60),
            root_edge_page: serializer::get_u32_le(buf, 64),
            root_label_page: serializer::get_u32_le(buf, 68),
            root_string_page: serializer::get_u32_le(buf, 72),
            freelist_head: serializer::get_u32_le(buf, 76),
            root_index_page: serializer::get_u32_le(buf, 80),
            schema_version: serializer::get_u32_le(buf, 84),
            checksum: serializer::get_u32_le(buf, 88),
            lsn: serializer::get_u32_le(buf, 92),
        }
    }

    /// Encodes the database metadata header into page 0 bytes.
    pub fn to_bytes(&self, buf: &mut [u8]) {
        buf[0..META_HEADER_SIZE].fill(0);
        buf[0..16].copy_from_slice(&self.magic);
        serializer::put_u32_le(buf, 16, self.version);
        serializer::put_u32_le(buf, 20, self.page_size);
        serializer::put_u32_le(buf, 24, self.db_size_pages);
        serializer::put_u64_le(buf, 28, self.node_count);
        serializer::put_u64_le(buf, 36, self.edge_count);
        serializer::put_u64_le(buf, 44, self.property_count);
        serializer::put_u64_le(buf, 52, self.label_count);
        serializer::put_u32_le(buf, 60, self.root_node_page);
        serializer::put_u32_le(buf, 64, self.root_edge_page);
        serializer::put_u32_le(buf, 68, self.root_label_page);
        serializer::put_u32_le(buf, 72, self.root_string_page);
        serializer::put_u32_le(buf, 76, self.freelist_head);
        serializer::put_u32_le(buf, 80, self.root_index_page);
        serializer::put_u32_le(buf, 84, self.schema_version);
        serializer::put_u32_le(buf, 88, self.checksum);
        serializer::put_u32_le(buf, 92, self.lsn);
    }
}

impl Default for MetaHeader {
    fn default() -> Self {
        Self::new()
    }
}

/// A 4-byte slot-table entry mapping a slot index to a record's offset and length within the page.
pub struct SlotEntry {
    /// Byte offset of the record payload within the page content area.
    pub offset: u16,
    /// Byte length of the record payload.
    pub length: u16,
}

impl SlotEntry {
    pub const DEAD: u16 = 0;

    /// Creates a slot-table entry pointing to one record payload inside a page.
    pub fn new(offset: u16, length: u16) -> Self {
        Self { offset, length }
    }

    /// Returns whether this slot has been deleted and no longer points to a record.
    #[inline]
    pub fn is_dead(&self) -> bool {
        self.offset == Self::DEAD
    }

    /// Decodes a slot-table entry from its 4-byte on-page representation.
    pub fn from_bytes(buf: &[u8]) -> Self {
        Self {
            offset: serializer::get_u16_le(buf, 0),
            length: serializer::get_u16_le(buf, 2),
        }
    }

    /// Encodes this slot-table entry into its 4-byte on-page representation.
    pub fn to_bytes(&self, buf: &mut [u8]) {
        serializer::put_u16_le(buf, 0, self.offset);
        serializer::put_u16_le(buf, 2, self.length);
    }
}

/// On-disk freelist page storing reusable page IDs in a linked list.
///
/// Layout: `[page_type: u8][padding..][next_page: u32][count: u16][page_id entries...]`
///
/// The `next_page` field points to the next freelist page (0 = end of chain).
/// Each entry is a 4-byte `PageId` referencing a freed page available for reuse.
pub struct FreelistPage {
    /// Page ID of the next freelist page, or 0 if this is the last page.
    pub next_page: u32,
    /// Page IDs of freed pages available for reuse.
    pub entries: Vec<u32>,
}

impl FreelistPage {
    /// Byte offset of the `next_page` field within a freelist page.
    pub const NEXT_PAGE_OFFSET: usize = 20;
    /// Byte offset of the `count` field within a freelist page.
    pub const COUNT_OFFSET: usize = 24;
    /// Byte offset where page ID entries begin.
    pub const DATA_OFFSET: usize = 26;
    /// Size in bytes of a single page ID entry.
    pub const ENTRY_SIZE: usize = 4;
    /// Maximum number of page IDs that fit in one freelist page.
    pub const MAX_ENTRIES: usize = (PAGE_SIZE - Self::DATA_OFFSET) / Self::ENTRY_SIZE;

    /// Creates an empty freelist page.
    pub fn new() -> Self {
        Self {
            next_page: 0,
            entries: Vec::new(),
        }
    }

    /// Decodes a freelist page from its on-disk byte representation.
    pub fn from_bytes(buf: &[u8]) -> Self {
        let next_page = serializer::get_u32_le(buf, Self::NEXT_PAGE_OFFSET);
        let count = serializer::get_u16_le(buf, Self::COUNT_OFFSET) as usize;
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let offset = Self::DATA_OFFSET + i * Self::ENTRY_SIZE;
            let page_id = serializer::get_u32_le(buf, offset);
            if page_id != 0 {
                entries.push(page_id);
            }
        }
        Self { next_page, entries }
    }

    /// Encodes this freelist page into its on-disk byte representation.
    pub fn to_bytes(&self, buf: &mut [u8]) {
        buf.fill(0);
        buf[0] = PageType::Freelist as u8;
        serializer::put_u32_le(buf, Self::NEXT_PAGE_OFFSET, self.next_page);
        serializer::put_u16_le(buf, Self::COUNT_OFFSET, self.entries.len() as u16);
        for (i, &page_id) in self.entries.iter().enumerate() {
            let offset = Self::DATA_OFFSET + i * Self::ENTRY_SIZE;
            serializer::put_u32_le(buf, offset, page_id);
        }
    }
}

impl Default for FreelistPage {
    fn default() -> Self {
        Self::new()
    }
}
