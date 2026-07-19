use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::db::hive_db::HiveDb;
use crate::storage::page::format::{HIVE_MAGIC, META_PAGE_ID, MetaHeader, PAGE_SIZE, is_meta_page};
use crate::storage::pager::Pager;

#[test]
fn pager_creates_meta_page_on_first_open() {
    let dir = temp_dir("pager_bootstrap");
    let mut pager = Pager::open(&dir, 16, 16).unwrap();

    assert_eq!(pager.page_count().unwrap(), 1);

    let page = pager.get_page(META_PAGE_ID).unwrap();
    assert!(is_meta_page(page));

    let meta = MetaHeader::from_bytes(page);
    assert_eq!(&meta.magic, &HIVE_MAGIC);
    assert_eq!(meta.version, 2);
    assert_eq!(meta.page_size as usize, PAGE_SIZE);
    assert_eq!(meta.db_size_pages, 1);
    assert_eq!(meta.node_count, 0);
    assert_eq!(meta.edge_count, 0);
    assert_eq!(meta.property_count, 0);

    drop(pager);
    cleanup_dir(&dir);
}

#[test]
fn pager_reopen_preserves_meta_page() {
    let dir = temp_dir("pager_reopen");

    {
        let mut pager = Pager::open(&dir, 16, 16).unwrap();
        let page = pager.get_page_mut(META_PAGE_ID).unwrap();
        let mut meta = MetaHeader::from_bytes(page);
        meta.node_count = 42;
        meta.edge_count = 7;
        meta.to_bytes(&mut *page);
        pager.mark_dirty(META_PAGE_ID).unwrap();
        pager.flush_all().unwrap();
    }

    {
        let mut pager = Pager::open(&dir, 16, 16).unwrap();
        let page = pager.get_page(META_PAGE_ID).unwrap();
        let meta = MetaHeader::from_bytes(page);
        assert_eq!(meta.node_count, 42);
        assert_eq!(meta.edge_count, 7);
    }

    cleanup_dir(&dir);
}

#[test]
fn hivedb_open_creates_valid_database() {
    let dir = temp_dir("hivedb_bootstrap");
    let db = HiveDb::open(&dir).unwrap();
    db.close();

    let mut pager = Pager::open(&dir, 16, 16).unwrap();
    let page = pager.get_page(META_PAGE_ID).unwrap();
    assert!(is_meta_page(page));

    let meta = MetaHeader::from_bytes(page);
    assert_eq!(&meta.magic, &HIVE_MAGIC);
    assert_eq!(meta.version, 2);
    assert_eq!(meta.page_size as usize, PAGE_SIZE);

    cleanup_dir(&dir);
}

#[test]
fn meta_page_is_pinned_in_cache() {
    let dir = temp_dir("meta_pinned");
    let mut pager = Pager::open(&dir, 16, 16).unwrap();

    pager.get_page(META_PAGE_ID).unwrap();
    assert!(pager.pin(META_PAGE_ID).is_ok());

    drop(pager);
    cleanup_dir(&dir);
}

#[test]
fn hivedb_open_on_existing_db_runs_recovery() {
    let dir = temp_dir("hivedb_recovery");

    {
        let db = HiveDb::open(&dir).unwrap();
        db.close();
    }

    {
        let db = HiveDb::open(&dir).unwrap();
        db.close();
    }

    cleanup_dir(&dir);
}
