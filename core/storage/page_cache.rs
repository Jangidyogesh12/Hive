use super::buffer_pool::{BufferPool, PageBuffer};
use super::page::format::{META_PAGE_ID, PAGE_SIZE};
use crate::errors::DbError;
use std::collections::{HashMap, VecDeque};

pub type PageId = u32;

/// One page currently resident in memory.
///
/// A cached page owns one 4KB buffer from the `BufferPool` and tracks the state
/// needed to decide whether the page can be safely evicted.
pub struct CachedPage {
    /// Logical page number in the database file.
    page_id: PageId,
    /// Raw 4KB page bytes.
    buffer: PageBuffer,
    /// Number of active users. A pinned page must not be evicted.
    pin_count: usize,
    /// SIEVE/clock reference bit. Reads set it; eviction clears it once.
    ref_bit: bool,
    /// True when memory has changes newer than the main database file.
    dirty: bool,
    /// True when a dirty page image has already been copied to WAL.
    spilled: bool,
}

impl CachedPage {
    /// Returns the logical page number stored in this cache entry.
    pub fn page_id(&self) -> PageId {
        self.page_id
    }

    /// Returns read-only access to the cached page bytes.
    pub fn data(&self) -> &[u8; PAGE_SIZE] {
        &self.buffer
    }

    /// Returns mutable page bytes and marks the page dirty.
    ///
    /// Any mutation makes the in-memory page newer than disk, so it must later
    /// be written to WAL or flushed before the cache can discard it safely.
    pub fn data_mut(&mut self) -> &mut [u8; PAGE_SIZE] {
        self.dirty = true;
        self.spilled = false;
        &mut self.buffer
    }

    /// Returns how many callers currently protect this page from eviction.
    pub fn pin_count(&self) -> usize {
        self.pin_count
    }

    /// Returns whether the cached page contains changes not yet written to disk.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Returns whether a dirty page image has already been copied to WAL.
    pub fn is_spilled(&self) -> bool {
        self.spilled
    }

    /// Marks the page as modified in memory.
    ///
    /// If it was previously spilled, that spill is no longer valid because the
    /// WAL contains an older image of the page.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.spilled = false;
    }

    /// Marks the page as matching durable storage again.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
        self.spilled = false;
    }

    /// Marks a dirty page as copied to WAL.
    ///
    /// The page may still be dirty relative to the main database file, but WAL
    /// recovery can restore it, so the cache may evict it if needed.
    pub fn mark_spilled(&mut self) {
        if self.dirty {
            self.spilled = true;
        }
    }

    /// A page is evictable only when no caller is using it and discarding the
    /// buffer will not lose changes.
    fn is_evictable(&self) -> bool {
        self.page_id != META_PAGE_ID && self.pin_count == 0 && (!self.dirty || self.spilled)
    }
}

/// Metadata returned when an entry leaves the cache.
pub struct EvictedPage {
    pub page_id: PageId,
    pub was_dirty: bool,
    pub was_spilled: bool,
}

/// In-memory mapping from page id to cached page buffer.
///
/// The cache owns page identity, pin/dirty state, and eviction policy. The
/// `BufferPool` only owns reusable memory; it does not know which page number is
/// stored in a buffer.
pub struct PageCache {
    capacity: usize,
    entries: HashMap<PageId, CachedPage>,
    /// Clock queue used by the SIEVE-style eviction pass.
    clock: VecDeque<PageId>,
}

impl PageCache {
    /// Creates an empty page cache with room for `capacity` pages.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::with_capacity(capacity),
            clock: VecDeque::with_capacity(capacity),
        }
    }

    /// Returns the maximum number of pages that can be resident in this cache.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of pages currently resident in memory.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the cache currently contains no pages.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns whether a page id already has a resident cache entry.
    pub fn contains(&self, page_id: PageId) -> bool {
        self.entries.contains_key(&page_id)
    }

    /// Looks up a cached page and sets its reference bit.
    pub fn get(&mut self, page_id: PageId) -> Option<&CachedPage> {
        let page = self.entries.get_mut(&page_id)?;
        page.ref_bit = true;
        Some(page)
    }

    /// Looks up a cached page mutably and sets its reference bit.
    pub fn get_mut(&mut self, page_id: PageId) -> Option<&mut CachedPage> {
        let page = self.entries.get_mut(&page_id)?;
        page.ref_bit = true;
        Some(page)
    }

    /// Prevents eviction while a caller is actively using the page.
    pub fn pin(&mut self, page_id: PageId) -> Result<(), DbError> {
        let page = self.entries.get_mut(&page_id).ok_or(DbError::ReadError)?;
        page.pin_count += 1;
        page.ref_bit = true;
        Ok(())
    }

    /// Releases one active use of a pinned page.
    pub fn unpin(&mut self, page_id: PageId) -> Result<(), DbError> {
        let page = self.entries.get_mut(&page_id).ok_or(DbError::ReadError)?;
        if page.pin_count == 0 {
            return Err(DbError::ReadError);
        }
        page.pin_count -= 1;
        Ok(())
    }

    /// Marks a cached page dirty after its contents were modified.
    pub fn mark_dirty(&mut self, page_id: PageId) -> Result<(), DbError> {
        let page = self.entries.get_mut(&page_id).ok_or(DbError::ReadError)?;
        page.mark_dirty();
        Ok(())
    }

    /// Marks a cached page clean after it has been written durably.
    pub fn mark_clean(&mut self, page_id: PageId) -> Result<(), DbError> {
        let page = self.entries.get_mut(&page_id).ok_or(DbError::ReadError)?;
        page.mark_clean();
        Ok(())
    }

    /// Marks a dirty page as safe to evict because its image exists in WAL.
    pub fn mark_spilled(&mut self, page_id: PageId) -> Result<(), DbError> {
        let page = self.entries.get_mut(&page_id).ok_or(DbError::ReadError)?;
        page.mark_spilled();
        Ok(())
    }

    /// Inserts a loaded page buffer into the cache.
    ///
    /// If the cache is full, this evicts one eligible page and returns its
    /// metadata. Page 0 starts pinned because it contains database metadata.
    pub fn insert(
        &mut self,
        page_id: PageId,
        buffer: PageBuffer,
        pool: &mut BufferPool,
    ) -> Result<Option<EvictedPage>, DbError> {
        if self.capacity == 0 {
            pool.release(buffer);
            return Err(DbError::WriteError);
        }

        if let Some(old_page) = self.entries.remove(&page_id) {
            pool.release(old_page.buffer);
            self.remove_from_clock(page_id);
        }

        let evicted = if self.entries.len() == self.capacity {
            match self.evict_one(pool) {
                Ok(evicted) => Some(evicted),
                Err(err) => {
                    pool.release(buffer);
                    return Err(err);
                }
            }
        } else {
            None
        };

        let pin_count = if page_id == META_PAGE_ID { 1 } else { 0 };
        self.entries.insert(
            page_id,
            CachedPage {
                page_id,
                buffer,
                pin_count,
                ref_bit: false,
                dirty: false,
                spilled: false,
            },
        );
        self.clock.push_back(page_id);

        Ok(evicted)
    }

    /// Runs one SIEVE-style eviction pass.
    ///
    /// Pages with the reference bit set get one second chance by clearing the
    /// bit. The first evictable page with a clear bit is removed.
    pub fn evict_one(&mut self, pool: &mut BufferPool) -> Result<EvictedPage, DbError> {
        if self.entries.is_empty() {
            return Err(DbError::ReadError);
        }

        let attempts = self.clock.len().saturating_mul(2);
        for _ in 0..attempts {
            let Some(page_id) = self.clock.pop_front() else {
                break;
            };

            let Some(page) = self.entries.get_mut(&page_id) else {
                continue;
            };

            if !page.is_evictable() {
                self.clock.push_back(page_id);
                continue;
            }

            if page.ref_bit {
                page.ref_bit = false;
                self.clock.push_back(page_id);
                continue;
            }

            let removed = self.entries.remove(&page_id).ok_or(DbError::ReadError)?;
            let evicted = EvictedPage {
                page_id,
                was_dirty: removed.dirty,
                was_spilled: removed.spilled,
            };
            pool.release(removed.buffer);
            return Ok(evicted);
        }

        Err(DbError::WriteError)
    }

    /// Returns ids for pages that must be written before shutdown/checkpoint.
    pub fn dirty_page_ids(&self) -> Vec<PageId> {
        self.entries
            .iter()
            .filter_map(|(page_id, page)| page.dirty.then_some(*page_id))
            .collect()
    }

    fn remove_from_clock(&mut self, page_id: PageId) {
        if let Some(pos) = self.clock.iter().position(|id| *id == page_id) {
            self.clock.remove(pos);
        }
    }
}

impl Default for PageCache {
    fn default() -> Self {
        Self::new(128)
    }
}
