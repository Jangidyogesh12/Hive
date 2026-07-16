use crate::storage::buffer_pool::BufferPool;
use crate::storage::page::format::PAGE_SIZE;
use crate::storage::page_cache::PageCache;

fn insert_page(cache: &mut PageCache, pool: &mut BufferPool, page_id: u32, marker: u8) {
    let mut buffer = pool.acquire().unwrap();
    buffer[0] = marker;
    cache.insert(page_id, buffer, pool).unwrap();
}

#[test]
fn cache_insert_and_get_returns_page_bytes() {
    let mut pool = BufferPool::new(2);
    let mut cache = PageCache::new(2);

    insert_page(&mut cache, &mut pool, 2, 77);

    let page = cache.get(2).unwrap();
    assert_eq!(page.page_id(), 2);
    assert_eq!(page.data()[0], 77);
    assert_eq!(cache.len(), 1);
    assert_eq!(pool.used(), 1);
}

#[test]
fn data_mut_marks_page_dirty() {
    let mut pool = BufferPool::new(1);
    let mut cache = PageCache::new(1);

    insert_page(&mut cache, &mut pool, 2, 0);
    let page = cache.get_mut(2).unwrap();
    page.data_mut()[PAGE_SIZE - 1] = 11;

    let page = cache.get(2).unwrap();
    assert!(page.is_dirty());
    assert!(!page.is_spilled());
    assert_eq!(page.data()[PAGE_SIZE - 1], 11);
}

#[test]
fn clean_unpinned_page_can_be_evicted() {
    let mut pool = BufferPool::new(2);
    let mut cache = PageCache::new(1);

    insert_page(&mut cache, &mut pool, 2, 20);
    let mut next = pool.acquire().unwrap();
    next[0] = 30;
    let evicted = cache.insert(3, next, &mut pool).unwrap().unwrap();

    assert_eq!(evicted.page_id, 2);
    assert!(cache.contains(3));
    assert!(!cache.contains(2));
    assert_eq!(pool.used(), 1);
}

#[test]
fn pinned_page_is_not_evicted() {
    let mut pool = BufferPool::new(2);
    let mut cache = PageCache::new(1);

    insert_page(&mut cache, &mut pool, 2, 20);
    cache.pin(2).unwrap();
    let next = pool.acquire().unwrap();

    assert!(cache.insert(3, next, &mut pool).is_err());
    assert!(cache.contains(2));
    assert_eq!(pool.used(), 1);
}

#[test]
fn meta_page_is_pinned_and_not_evicted() {
    let mut pool = BufferPool::new(2);
    let mut cache = PageCache::new(1);

    insert_page(&mut cache, &mut pool, 1, 1);
    assert_eq!(cache.get(1).unwrap().pin_count(), 1);
    let next = pool.acquire().unwrap();

    assert!(cache.insert(2, next, &mut pool).is_err());
    assert!(cache.contains(1));
    assert_eq!(pool.used(), 1);
}

#[test]
fn dirty_page_is_not_evicted_until_spilled() {
    let mut pool = BufferPool::new(2);
    let mut cache = PageCache::new(1);

    insert_page(&mut cache, &mut pool, 2, 20);
    cache.mark_dirty(2).unwrap();
    let next = pool.acquire().unwrap();

    assert!(cache.insert(3, next, &mut pool).is_err());
    assert!(cache.contains(2));
    assert_eq!(pool.used(), 1);
}

#[test]
fn spilled_dirty_page_can_be_evicted() {
    let mut pool = BufferPool::new(2);
    let mut cache = PageCache::new(1);

    insert_page(&mut cache, &mut pool, 2, 20);
    cache.mark_dirty(2).unwrap();
    cache.mark_spilled(2).unwrap();
    let next = pool.acquire().unwrap();
    let evicted = cache.insert(3, next, &mut pool).unwrap().unwrap();

    assert_eq!(evicted.page_id, 2);
    assert!(evicted.was_dirty);
    assert!(evicted.was_spilled);
    assert!(cache.contains(3));
}
