// Tests for PageType, PageHeader, MetaHeader, and SlotEntry serialization.
use crate::storage::page::format::{self, MetaHeader, PageHeader, PageType, SlotEntry};
use crate::storage::page::format::{
    META_HEADER_SIZE, PAGE_SIZE, REGULAR_HEADER_SIZE, SLOT_ENTRY_SIZE,
};

#[test]
fn page_type_from_u8_maps_all_valid_codes() {
    let mappings = [
        (0x00, PageType::Meta),
        (0x01, PageType::DataNode),
        (0x02, PageType::DataEdge),
        (0x03, PageType::DataProperty),
        (0x04, PageType::StringData),
        (0x05, PageType::LabelData),
        (0x0A, PageType::IndexInterior),
        (0x0B, PageType::IndexLeaf),
        (0x0F, PageType::Freelist),
        (0x10, PageType::Overflow),
    ];

    for (code, expected) in &mappings {
        let decoded = PageType::from_u8(*code).unwrap();
        assert_eq!(decoded, *expected, "failed for code 0x{:02X}", code);
        assert_eq!(decoded as u8, *code);
    }
}

#[test]
fn page_type_from_u8_returns_none_for_invalid_codes() {
    let invalid = [0x06, 0x07, 0x08, 0x09, 0x0C, 0x0D, 0x0E, 0x11, 0x20, 0xFF];
    for code in &invalid {
        assert!(
            PageType::from_u8(*code).is_none(),
            "0x{:02X} should be invalid",
            code
        );
    }
}

#[test]
fn page_header_new_sets_defaults() {
    let header = PageHeader::new(PageType::DataNode);
    assert_eq!(header.page_type, PageType::DataNode);
    assert_eq!(header.free_flags, 0);
    assert_eq!(header.slot_count, 0);
    assert_eq!(header.free_space_offset as usize, PAGE_SIZE);
    assert_eq!(header.first_freeblock, 0);
    assert_eq!(header.checksum, 0);
    assert_eq!(header.lsn, 0);
}

#[test]
fn page_header_byte_roundtrip() {
    let header = PageHeader {
        page_type: PageType::DataEdge,
        free_flags: PageHeader::HAS_OVERFLOW | PageHeader::IS_COMPRESSED,
        slot_count: 100,
        free_space_offset: 3500,
        first_freeblock: 2800,
        checksum: 0xABCD1234,
        lsn: 42,
        reserved: 99,
    };

    let mut buf = [0u8; REGULAR_HEADER_SIZE];
    header.to_bytes(&mut buf);

    let decoded = PageHeader::from_bytes(&buf);
    assert_eq!(decoded.page_type, PageType::DataEdge);
    assert_eq!(decoded.free_flags, 0x03);
    assert_eq!(decoded.slot_count, 100);
    assert_eq!(decoded.free_space_offset, 3500);
    assert_eq!(decoded.first_freeblock, 2800);
    assert_eq!(decoded.checksum, 0xABCD1234);
    assert_eq!(decoded.lsn, 42);
    assert_eq!(decoded.reserved, 99);
}

#[test]
fn page_header_roundtrip_all_types() {
    let types = [
        PageType::DataNode,
        PageType::DataEdge,
        PageType::DataProperty,
        PageType::StringData,
        PageType::LabelData,
        PageType::IndexInterior,
        PageType::IndexLeaf,
        PageType::Freelist,
        PageType::Overflow,
    ];

    for &pt in &types {
        let mut header = PageHeader::new(pt);
        header.slot_count = 5;
        header.lsn = 10;

        let mut buf = [0u8; REGULAR_HEADER_SIZE];
        header.to_bytes(&mut buf);
        let decoded = PageHeader::from_bytes(&buf);
        assert_eq!(decoded.page_type, pt);
        assert_eq!(decoded.slot_count, 5);
        assert_eq!(decoded.lsn, 10);
    }
}

#[test]
fn meta_header_new_sets_defaults() {
    let meta = MetaHeader::new();
    assert_eq!(&meta.magic[0..4], b"HIVE");
    assert_eq!(meta.version, format::CURRENT_VERSION);
    assert_eq!(meta.page_size as usize, PAGE_SIZE);
    assert_eq!(meta.db_size_pages, 1);
    assert_eq!(meta.node_count, 0);
    assert_eq!(meta.edge_count, 0);
    assert_eq!(meta.property_count, 0);
    assert_eq!(meta.root_data_page, 0);
    assert_eq!(meta.freelist_head, 0);
}

#[test]
fn meta_header_byte_roundtrip() {
    let mut meta = MetaHeader::new();
    meta.db_size_pages = 50;
    meta.node_count = 1000;
    meta.edge_count = 500;
    meta.property_count = 2000;
    meta.root_data_page = 2;
    meta.root_edge_page = 25;
    meta.freelist_head = 30;
    meta.schema_version = 3;
    meta.lsn = 7;

    let mut buf = [0u8; META_HEADER_SIZE];
    meta.to_bytes(&mut buf);

    let decoded = MetaHeader::from_bytes(&buf);
    assert_eq!(decoded.db_size_pages, 50);
    assert_eq!(decoded.node_count, 1000);
    assert_eq!(decoded.edge_count, 500);
    assert_eq!(decoded.property_count, 2000);
    assert_eq!(decoded.root_data_page, 2);
    assert_eq!(decoded.root_edge_page, 25);
    assert_eq!(decoded.freelist_head, 30);
    assert_eq!(decoded.schema_version, 3);
    assert_eq!(decoded.lsn, 7);
}

#[test]
fn meta_header_from_bytes_handles_all_defaults() {
    let meta = MetaHeader::new();
    let mut buf = [0u8; META_HEADER_SIZE];
    meta.to_bytes(&mut buf);
    let decoded = MetaHeader::from_bytes(&buf);

    assert_eq!(&decoded.magic, &format::HIVE_MAGIC);
    assert_eq!(decoded.version, format::CURRENT_VERSION);
    assert_eq!(decoded.page_size as usize, PAGE_SIZE);
}

#[test]
fn slot_entry_new_stores_correct_values() {
    let slot = SlotEntry::new(3800, 64);
    assert_eq!(slot.offset, 3800);
    assert_eq!(slot.length, 64);
    assert!(!slot.is_dead());
}

#[test]
fn slot_entry_dead_detection() {
    let dead = SlotEntry::new(SlotEntry::DEAD, 0);
    assert!(dead.is_dead());

    let alive = SlotEntry::new(100, 10);
    assert!(!alive.is_dead());
}

#[test]
fn slot_entry_byte_roundtrip() {
    let slot = SlotEntry::new(3900, 128);
    let mut buf = [0u8; SLOT_ENTRY_SIZE];
    slot.to_bytes(&mut buf);

    let decoded = SlotEntry::from_bytes(&buf);
    assert_eq!(decoded.offset, 3900);
    assert_eq!(decoded.length, 128);
    assert!(!decoded.is_dead());
}

#[test]
fn slot_entry_dead_roundtrip() {
    let dead = SlotEntry::new(SlotEntry::DEAD, 0);
    let mut buf = [0u8; SLOT_ENTRY_SIZE];
    dead.to_bytes(&mut buf);

    let decoded = SlotEntry::from_bytes(&buf);
    assert_eq!(decoded.offset, SlotEntry::DEAD);
    assert_eq!(decoded.length, 0);
    assert!(decoded.is_dead());
}

#[test]
fn page_size_is_4096() {
    assert_eq!(PAGE_SIZE, 4096);
}

#[test]
fn regular_header_size_is_20() {
    assert_eq!(REGULAR_HEADER_SIZE, 20);
}

#[test]
fn meta_header_size_is_100() {
    assert_eq!(META_HEADER_SIZE, 100);
}

#[test]
fn slot_entry_size_is_4() {
    assert_eq!(SLOT_ENTRY_SIZE, 4);
}
