/// On-disk page format definitions: page types, page header, meta header, and constants.
use super::serializer;

pub const PAGE_SIZE: usize = 4096;
pub const REGULAR_HEADER_SIZE: usize = 20;
pub const META_HEADER_SIZE: usize = 100;
pub const SLOT_ENTRY_SIZE: usize = 4;
pub const HIVE_MAGIC: [u8; 16] = [b'H', b'I', b'V', b'E', 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
pub const CURRENT_VERSION: u32 = 2;
pub const META_PAGE_ID: u32 = 0;

/// Returns true if the page buffer starts with the Hive magic bytes.
pub fn is_meta_page(buf: &[u8; PAGE_SIZE]) -> bool {
    buf[..16] == HIVE_MAGIC
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageType {
    Meta = 0x00,
    DataNode = 0x01,
    DataEdge = 0x02,
    DataProperty = 0x03,
    StringData = 0x04,
    LabelData = 0x05,
    IndexInterior = 0x0A,
    IndexLeaf = 0x0B,
    Freelist = 0x0F,
    Overflow = 0x10,
}

impl PageType {
    /// Converts the on-disk page type byte into a known page type.
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Self::Meta),
            0x01 => Some(Self::DataNode),
            0x02 => Some(Self::DataEdge),
            0x03 => Some(Self::DataProperty),
            0x04 => Some(Self::StringData),
            0x05 => Some(Self::LabelData),
            0x0A => Some(Self::IndexInterior),
            0x0B => Some(Self::IndexLeaf),
            0x0F => Some(Self::Freelist),
            0x10 => Some(Self::Overflow),
            _ => None,
        }
    }
}

pub struct PageHeader {
    pub page_type: PageType,
    pub free_flags: u8,
    pub slot_count: u16,
    pub free_space_offset: u16,
    pub first_freeblock: u16,
    pub checksum: u32,
    pub lsn: u32,
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

pub struct MetaHeader {
    pub magic: [u8; 16],
    pub version: u32,
    pub page_size: u32,
    pub db_size_pages: u32,
    pub node_count: u64,
    pub edge_count: u64,
    pub property_count: u64,
    pub label_count: u64,
    pub root_node_page: u32,
    pub root_edge_page: u32,
    pub root_label_page: u32,
    pub root_string_page: u32,
    pub freelist_head: u32,
    pub schema_version: u32,
    pub checksum: u32,
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
            schema_version: serializer::get_u32_le(buf, 80),
            checksum: serializer::get_u32_le(buf, 84),
            lsn: serializer::get_u32_le(buf, 88),
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
        serializer::put_u32_le(buf, 80, self.schema_version);
        serializer::put_u32_le(buf, 84, self.checksum);
        serializer::put_u32_le(buf, 88, self.lsn);
    }
}

impl Default for MetaHeader {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SlotEntry {
    pub offset: u16,
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
