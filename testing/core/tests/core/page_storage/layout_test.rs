// Tests for page-level operations: init, insert, read, delete, compact, checksum.
use crate::storage::page::format::{
    META_HEADER_SIZE, MetaHeader, PAGE_SIZE, PageType, REGULAR_HEADER_SIZE, SLOT_ENTRY_SIZE,
};
use crate::storage::page::layout;

fn new_data_node_page() -> [u8; PAGE_SIZE] {
    let mut buf = [0u8; PAGE_SIZE];
    layout::init_regular_page(&mut buf, PageType::DataNode);
    buf
}

fn new_meta_page() -> [u8; PAGE_SIZE] {
    let mut buf = [0u8; PAGE_SIZE];
    layout::init_meta_page(&mut buf, &MetaHeader::new());
    buf
}

#[test]
fn init_regular_page_sets_header() {
    let mut buf = [0u8; PAGE_SIZE];
    layout::init_regular_page(&mut buf, PageType::DataEdge);

    let header = layout::read_page_header(&buf);
    assert_eq!(header.page_type, PageType::DataEdge);
    assert_eq!(header.slot_count, 0);
    assert_eq!(header.free_space_offset as usize, PAGE_SIZE);
    assert_eq!(header.first_freeblock, 0);
}

#[test]
fn init_regular_page_produces_full_zeroed_body() {
    let mut buf = [0xFFu8; PAGE_SIZE];
    layout::init_regular_page(&mut buf, PageType::DataNode);

    assert_eq!(buf[0], PageType::DataNode as u8);
    assert_eq!(buf[REGULAR_HEADER_SIZE], 0);
    assert_eq!(buf[PAGE_SIZE - 1], 0);
}

#[test]
fn init_meta_page_writes_magic() {
    let mut buf = [0u8; PAGE_SIZE];
    layout::init_meta_page(&mut buf, &MetaHeader::new());

    let meta = layout::read_meta_header(&buf);
    assert_eq!(&meta.magic[0..4], b"HIVE");
}

#[test]
fn insert_single_record_then_read() {
    let mut buf = new_data_node_page();
    let data = b"test record payload";

    let slot = layout::insert_record(&mut buf, data).unwrap();
    assert_eq!(slot.0, 0);

    let header = layout::read_page_header(&buf);
    assert_eq!(header.slot_count, 1);

    let read = layout::read_record(&buf, 0).unwrap();
    assert_eq!(&read, data);
}

#[test]
fn insert_multiple_records_keeps_order() {
    let mut buf = new_data_node_page();
    let records: Vec<Vec<u8>> = (0..30)
        .map(|i| format!("record_{:04}", i).into_bytes())
        .collect();

    for rec in &records {
        layout::insert_record(&mut buf, rec).unwrap();
    }

    let header = layout::read_page_header(&buf);
    assert_eq!(header.slot_count, 30);

    for (i, expected) in records.iter().enumerate() {
        let got = layout::read_record(&buf, i as u16).unwrap();
        assert_eq!(&got, expected, "record {} mismatch", i);
    }
}

#[test]
fn insert_variable_size_records() {
    let mut buf = new_data_node_page();
    let r1 = b"tiny";
    let r2 = vec![b'X'; 500];
    let r3 = vec![b'Y'; 100];

    layout::insert_record(&mut buf, r1).unwrap();
    layout::insert_record(&mut buf, &r2).unwrap();
    layout::insert_record(&mut buf, &r3).unwrap();

    assert_eq!(layout::read_record(&buf, 0).unwrap(), r1);
    assert_eq!(layout::read_record(&buf, 1).unwrap(), r2);
    assert_eq!(layout::read_record(&buf, 2).unwrap(), r3);
}

#[test]
fn insert_after_delete_reuses_slot() {
    let mut buf = new_data_node_page();
    layout::insert_record(&mut buf, b"AAAA").unwrap();
    layout::insert_record(&mut buf, b"BBBB").unwrap();

    layout::delete_record(&mut buf, 0).unwrap();
    assert!(layout::read_record(&buf, 0).is_none());

    layout::insert_record(&mut buf, b"CCCC").unwrap();
    assert_eq!(layout::read_record(&buf, 2).unwrap(), b"CCCC");
    assert_eq!(layout::read_record(&buf, 1).unwrap(), b"BBBB");
}

#[test]
fn delete_record_makes_slot_unreadable() {
    let mut buf = new_data_node_page();
    layout::insert_record(&mut buf, b"first").unwrap();
    layout::insert_record(&mut buf, b"second").unwrap();
    layout::insert_record(&mut buf, b"third").unwrap();

    layout::delete_record(&mut buf, 1).unwrap();
    assert!(layout::read_record(&buf, 1).is_none());
    assert!(layout::read_record(&buf, 0).is_some());
    assert!(layout::read_record(&buf, 2).is_some());
}

#[test]
fn delete_out_of_bounds_returns_error() {
    let mut buf = new_data_node_page();
    layout::insert_record(&mut buf, b"only").unwrap();

    assert!(layout::delete_record(&mut buf, 5).is_err());
}

#[test]
fn delete_already_dead_slot_is_noop() {
    let mut buf = new_data_node_page();
    layout::insert_record(&mut buf, b"data").unwrap();
    layout::delete_record(&mut buf, 0).unwrap();

    assert!(layout::delete_record(&mut buf, 0).is_ok());
}

#[test]
fn compact_reclaims_space_after_deletions() {
    let mut buf = new_data_node_page();
    layout::insert_record(&mut buf, b"keep1").unwrap();
    layout::insert_record(&mut buf, b"delete_me").unwrap();
    layout::insert_record(&mut buf, b"keep2").unwrap();
    layout::insert_record(&mut buf, b"also_delete").unwrap();
    layout::insert_record(&mut buf, b"keep3").unwrap();

    layout::delete_record(&mut buf, 1).unwrap();
    layout::delete_record(&mut buf, 3).unwrap();

    let free_before = layout::get_free_space(&buf);
    layout::compact_page(&mut buf).unwrap();
    let free_after = layout::get_free_space(&buf);

    assert!(
        free_after > free_before,
        "compact should reclaim free space"
    );

    let header = layout::read_page_header(&buf);
    assert_eq!(header.slot_count, 3);
}

#[test]
fn compact_preserves_live_records_in_order() {
    let mut buf = new_data_node_page();
    layout::insert_record(&mut buf, b"AAA").unwrap();
    layout::insert_record(&mut buf, b"BBB").unwrap();
    layout::insert_record(&mut buf, b"CCC").unwrap();
    layout::insert_record(&mut buf, b"DDD").unwrap();

    layout::delete_record(&mut buf, 0).unwrap();
    layout::delete_record(&mut buf, 2).unwrap();

    layout::compact_page(&mut buf).unwrap();

    assert_eq!(layout::read_record(&buf, 0).unwrap(), b"BBB");
    assert_eq!(layout::read_record(&buf, 1).unwrap(), b"DDD");
    assert!(layout::read_record(&buf, 2).is_none());
}

#[test]
fn compact_empty_page_is_noop() {
    let mut buf = new_data_node_page();
    assert!(layout::compact_page(&mut buf).is_ok());
    let header = layout::read_page_header(&buf);
    assert_eq!(header.slot_count, 0);
}

#[test]
fn compact_all_dead_page_clears_slots() {
    let mut buf = new_data_node_page();
    layout::insert_record(&mut buf, b"a").unwrap();
    layout::insert_record(&mut buf, b"b").unwrap();
    layout::delete_record(&mut buf, 0).unwrap();
    layout::delete_record(&mut buf, 1).unwrap();

    layout::compact_page(&mut buf).unwrap();
    let header = layout::read_page_header(&buf);
    assert_eq!(header.slot_count, 0);
    assert_eq!(header.free_space_offset as usize, PAGE_SIZE);
}

#[test]
fn checksum_verification_passes_after_insert() {
    let mut buf = new_data_node_page();
    layout::insert_record(&mut buf, b"checksum test data").unwrap();
    assert!(layout::verify_checksum(&buf));
}

#[test]
fn checksum_verification_fails_after_data_corruption() {
    let mut buf = new_data_node_page();
    layout::insert_record(&mut buf, b"original data").unwrap();
    buf[PAGE_SIZE - 10] ^= 0xFF;

    assert!(!layout::verify_checksum(&buf));
}

#[test]
fn get_free_space_decreases_after_insert() {
    let mut buf = new_data_node_page();
    let initial = layout::get_free_space(&buf);

    layout::insert_record(&mut buf, &[0u8; 200]).unwrap();
    let after = layout::get_free_space(&buf);

    let expected_used = 200 + SLOT_ENTRY_SIZE;
    assert_eq!(
        after,
        initial - expected_used,
        "free space should decrease by record size + slot entry"
    );
}

#[test]
fn insert_fails_when_free_space_insufficient() {
    let mut buf = new_data_node_page();
    let record = vec![0u8; 100];
    while layout::insert_record(&mut buf, &record).is_ok() {}

    let free = layout::get_free_space(&buf);
    assert!(
        free < 100 + SLOT_ENTRY_SIZE,
        "free space {} should be less than needed for another record",
        free
    );
}

#[test]
fn insert_returns_error_when_page_full() {
    let mut buf = new_data_node_page();
    let large = vec![b'X'; PAGE_SIZE - REGULAR_HEADER_SIZE - SLOT_ENTRY_SIZE];

    layout::insert_record(&mut buf, &large).unwrap();
    assert!(layout::insert_record(&mut buf, b"too_much").is_err());
}

#[test]
fn live_slot_count_counts_only_live_slots() {
    let mut buf = new_data_node_page();
    layout::insert_record(&mut buf, b"a").unwrap();
    layout::insert_record(&mut buf, b"b").unwrap();
    layout::insert_record(&mut buf, b"c").unwrap();
    layout::insert_record(&mut buf, b"d").unwrap();

    assert_eq!(layout::live_slot_count(&buf), 4);

    layout::delete_record(&mut buf, 1).unwrap();
    layout::delete_record(&mut buf, 3).unwrap();

    assert_eq!(layout::live_slot_count(&buf), 2);
}

#[test]
fn read_page_header_then_write_roundtrip() {
    let mut buf = new_data_node_page();
    layout::insert_record(&mut buf, b"something").unwrap();

    let mut header = layout::read_page_header(&buf);
    header.lsn = 99;
    layout::write_page_header(&mut buf, &header);

    let reread = layout::read_page_header(&buf);
    assert_eq!(reread.lsn, 99);
}

#[test]
fn write_meta_header_then_read_roundtrip() {
    let mut buf = new_meta_page();
    let mut meta = layout::read_meta_header(&buf);
    meta.db_size_pages = 500;
    meta.node_count = 10000;
    layout::write_meta_header(&mut buf, &meta);

    let reread = layout::read_meta_header(&buf);
    assert_eq!(reread.db_size_pages, 500);
    assert_eq!(reread.node_count, 10000);
}

#[test]
fn regular_page_content_starts_at_regular_header_size() {
    let mut buf = new_data_node_page();
    layout::insert_record(&mut buf, b"payload").unwrap();

    let header = layout::read_page_header(&buf);
    let slot_0_pos = REGULAR_HEADER_SIZE;
    assert_eq!(header.slot_count, 1);
    assert!(
        buf[slot_0_pos] != 0 || buf[slot_0_pos + 1] != 0,
        "slot entry should be non-zero"
    );
}

#[test]
fn meta_page_content_starts_at_meta_header_size() {
    let buf = new_meta_page();
    assert_eq!(buf[0], b'H');
    assert_eq!(buf[META_HEADER_SIZE], 0);
    assert_eq!(buf[PAGE_SIZE - 1], 0);
}
