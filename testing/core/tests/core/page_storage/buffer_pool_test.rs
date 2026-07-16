use crate::storage::buffer_pool::BufferPool;
use crate::storage::page::format::PAGE_SIZE;

#[test]
fn buffer_pool_acquires_until_capacity() {
    let mut pool = BufferPool::new(2);

    let first = pool.acquire();
    let second = pool.acquire();
    let third = pool.acquire();

    assert!(first.is_some());
    assert!(second.is_some());
    assert!(third.is_none());
    assert_eq!(pool.used(), 2);
    assert_eq!(pool.available(), 0);
}

#[test]
fn buffer_pool_recycles_zeroed_buffers() {
    let mut pool = BufferPool::new(1);
    let mut buffer = pool.acquire().unwrap();
    buffer[0] = 99;
    buffer[PAGE_SIZE - 1] = 42;

    pool.release(buffer);
    let buffer = pool.acquire().unwrap();

    assert_eq!(buffer[0], 0);
    assert_eq!(buffer[PAGE_SIZE - 1], 0);
    assert_eq!(pool.used(), 1);
}
