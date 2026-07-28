use super::buffer_pool::BufferPool;
use super::page::format::{FreelistPage, META_PAGE_ID, MetaHeader, PAGE_SIZE};
use super::page::layout;
use super::page_cache::{PageCache, PageId};
use crate::errors::DbError;
use std::fs::{File, OpenOptions, create_dir_all};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub type Lsn = u64;

pub const DB_FILE: &str = "hive.db";

struct FileHandle {
    reader: File,
    writer: BufWriter<File>,
}

impl FileHandle {
    /// Opens the database file with separate handles for buffered writes and positioned reads.
    fn open(path: &Path) -> Result<Self, DbError> {
        let reader = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;
        let writer_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(|_| DbError::FileOpenError)?;
        Ok(Self {
            reader,
            writer: BufWriter::new(writer_file),
        })
    }

    /// Reads exactly one page from its byte offset in the database file.
    fn read_page(&mut self, page_id: PageId, buf: &mut [u8; PAGE_SIZE]) -> Result<(), DbError> {
        self.writer.flush().map_err(|_| DbError::WriteError)?;
        let offset = (page_id as u64) * (PAGE_SIZE as u64);
        self.reader
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::SeekError)?;
        self.reader
            .read_exact(buf)
            .map_err(|_| DbError::ReadError)?;
        Ok(())
    }

    /// Writes exactly one page to its byte offset in the database file.
    fn write_page(&mut self, page_id: PageId, buf: &[u8; PAGE_SIZE]) -> Result<(), DbError> {
        let offset = (page_id as u64) * (PAGE_SIZE as u64);
        self.writer
            .seek(SeekFrom::Start(offset))
            .map_err(|_| DbError::SeekError)?;
        self.writer
            .write_all(buf)
            .map_err(|_| DbError::WriteError)?;
        Ok(())
    }

    /// Flushes buffered bytes from the writer into the operating system.
    fn flush(&mut self) -> Result<(), DbError> {
        self.writer.flush().map_err(|_| DbError::WriteError)
    }

    /// Flushes buffered bytes and asks the OS to persist them to durable storage.
    fn sync(&mut self) -> Result<(), DbError> {
        self.flush()?;
        self.writer.get_ref().sync_all().map_err(DbError::Io)
    }

    /// Returns the current database file size after flushing pending buffered writes.
    fn file_size(&mut self) -> Result<u64, DbError> {
        self.flush()?;
        self.reader
            .seek(SeekFrom::End(0))
            .map_err(|_| DbError::SeekError)
    }
}

pub struct Pager {
    /// Low-level file I/O handle for the database file.
    file: FileHandle,
    /// In-memory cache of recently accessed pages (LRU eviction).
    page_cache: PageCache,
    /// Reusable buffer pool to avoid repeated heap allocations for page I/O.
    pool: BufferPool,
    /// Monotonically increasing log sequence number for ordering page changes (WAL).
    next_lsn: AtomicU64,
    /// In-memory stack of page IDs available for reuse (pop on alloc, push on free).
    free_pages: Vec<PageId>,
    /// Page ID of the first freelist page on disk (linked list head), or 0 if none.
    freelist_head: PageId,
    /// True if free_pages changed since last persist to disk.
    freelist_dirty: bool,
}

impl Pager {
    /// Opens the pager for `hive.db` and creates its cache and reusable buffer pool.
    ///
    /// If the database file is empty, this initializes page 0 as the meta page.
    /// Loads the persistent freelist from disk if present.
    pub fn open(
        db_dir: &Path,
        cache_capacity: usize,
        pool_capacity: usize,
    ) -> Result<Self, DbError> {
        create_dir_all(db_dir).map_err(|_| DbError::FileOpenError)?;

        let path = db_dir.join(DB_FILE);
        let file = FileHandle::open(&path)?;

        let page_cache = PageCache::new(cache_capacity);
        let pool = BufferPool::new(pool_capacity);

        let mut pager = Self {
            file,
            page_cache,
            pool,
            next_lsn: AtomicU64::new(1),
            free_pages: Vec::new(),
            freelist_head: 0,
            freelist_dirty: false,
        };

        let page_count = pager.page_count()?;
        if page_count == 0 {
            pager.bootstrap_new_db()?;
        } else {
            pager.load_freelist()?;
        }

        Ok(pager)
    }

    /// Initializes a brand-new database by writing a valid meta page to page 0.
    fn bootstrap_new_db(&mut self) -> Result<(), DbError> {
        let mut buf = [0u8; PAGE_SIZE];
        let meta = MetaHeader::new();
        layout::init_meta_page(&mut buf, &meta);

        self.file.write_page(META_PAGE_ID, &buf)?;
        self.file.flush()?;
        self.file.sync()?;

        let buf_owned = Box::new(buf);
        if let Some(evicted) = self
            .page_cache
            .insert(META_PAGE_ID, buf_owned, &mut self.pool)?
            && evicted.was_dirty
        {
            self.flush_page_to_disk(evicted.page_id)?;
        }

        Ok(())
    }

    /// Loads the persistent freelist from disk into the in-memory free_pages Vec.
    fn load_freelist(&mut self) -> Result<(), DbError> {
        let mut meta_buf = [0u8; PAGE_SIZE];
        self.file.read_page(META_PAGE_ID, &mut meta_buf)?;
        let meta = layout::read_meta_header(&meta_buf);
        self.freelist_head = meta.freelist_head;

        let mut current = self.freelist_head;
        while current != 0 {
            let mut page_buf = [0u8; PAGE_SIZE];
            self.file.read_page(current, &mut page_buf)?;
            let flp = FreelistPage::from_bytes(&page_buf);
            self.free_pages.extend(flp.entries);
            current = flp.next_page;
        }

        Ok(())
    }

    /// Allocates a new log sequence number for ordering page changes.
    pub fn next_lsn(&self) -> Lsn {
        self.next_lsn.fetch_add(1, Ordering::SeqCst)
    }

    /// Returns the next log sequence number that will be handed out.
    pub fn current_lsn(&self) -> Lsn {
        self.next_lsn.load(Ordering::SeqCst)
    }

    /// Restores the next log sequence number after recovery or metadata loading.
    pub fn set_next_lsn(&self, lsn: Lsn) {
        self.next_lsn.store(lsn, Ordering::SeqCst);
    }

    /// Counts how many fixed-size pages currently exist in the database file.
    pub fn page_count(&mut self) -> Result<u64, DbError> {
        let size = self.file.file_size()?;
        Ok(size / PAGE_SIZE as u64)
    }

    /// Reads a page through the cache and returns an owned copy of its bytes.
    ///
    /// If the page is not cached, it is loaded from disk and inserted into the
    /// page cache before the copy is returned.
    pub fn read_page(&mut self, page_id: PageId) -> Result<[u8; PAGE_SIZE], DbError> {
        if let Some(cached) = self.page_cache.get(page_id) {
            return Ok(*cached.data());
        }

        let mut buf = self.pool.acquire().ok_or(DbError::WriteError)?;
        self.file.read_page(page_id, &mut buf)?;
        let buf_owned = Box::new(*buf);

        if let Some(evicted) = self.page_cache.insert(page_id, buf_owned, &mut self.pool)?
            && evicted.was_dirty
        {
            self.flush_page_to_disk(evicted.page_id)?;
        }

        Ok(*self.page_cache.get(page_id).unwrap().data())
    }

    /// Returns a read-only reference to a cached page, loading it from disk if needed.
    ///
    /// Use this when the caller only needs to inspect page bytes and must not
    /// modify the page contents.
    pub fn get_page(&mut self, page_id: PageId) -> Result<&[u8; PAGE_SIZE], DbError> {
        if !self.page_cache.contains(page_id) {
            self.read_page(page_id)?;
        }
        Ok(self
            .page_cache
            .get(page_id)
            .ok_or(DbError::ReadError)?
            .data())
    }

    /// Returns a mutable reference to a cached page, loading it from disk if needed.
    ///
    /// Mutating through this reference marks the cached page dirty, so it must be
    /// flushed or written to WAL before eviction.
    pub fn get_page_mut(&mut self, page_id: PageId) -> Result<&mut [u8; PAGE_SIZE], DbError> {
        if !self.page_cache.contains(page_id) {
            self.read_page(page_id)?;
        }
        Ok(self
            .page_cache
            .get_mut(page_id)
            .ok_or(DbError::ReadError)?
            .data_mut())
    }

    /// Marks a cached page as modified when the caller changed it outside `data_mut`.
    pub fn mark_dirty(&mut self, page_id: PageId) -> Result<(), DbError> {
        self.page_cache.mark_dirty(page_id)
    }

    /// Stamps a page's header LSN to the given value.
    ///
    /// This is called after writing a PageImage to the WAL so that recovery
    /// can compare the on-disk page LSN against the WAL entry's page_lsn.
    pub fn stamp_page_lsn(&mut self, page_id: PageId, lsn: Lsn) -> Result<(), DbError> {
        let page = self.get_page_mut(page_id)?;
        let mut header = super::page::format::PageHeader::from_bytes(page);
        header.lsn = lsn as u32;
        header.to_bytes(page);
        Ok(())
    }

    /// Marks a cached page as spilled (safe to evict because its image is in WAL).
    pub fn mark_spilled(&mut self, page_id: PageId) -> Result<(), DbError> {
        self.page_cache.mark_spilled(page_id)
    }

    /// Marks a cached page clean after restoring it to an already-durable image.
    pub fn mark_clean(&mut self, page_id: PageId) -> Result<(), DbError> {
        self.page_cache.mark_clean(page_id)
    }

    /// Restores a cached page to the provided bytes.
    pub fn restore_page(&mut self, page_id: PageId, data: &[u8; PAGE_SIZE]) -> Result<(), DbError> {
        let page = self.get_page_mut(page_id)?;
        page.copy_from_slice(data);
        Ok(())
    }

    /// Increments a page's pin count so cache eviction cannot remove it while in use.
    pub fn pin(&mut self, page_id: PageId) -> Result<(), DbError> {
        self.page_cache.pin(page_id)
    }

    /// Decrements a page's pin count after the caller is done using it.
    pub fn unpin(&mut self, page_id: PageId) -> Result<(), DbError> {
        self.page_cache.unpin(page_id)
    }

    /// Returns ids of cached pages whose in-memory bytes are newer than disk.
    pub fn dirty_page_ids(&self) -> Vec<PageId> {
        self.page_cache.dirty_page_ids()
    }

    /// Appends a new zero-filled page to the database file and returns its page id.
    pub fn allocate_page(&mut self) -> Result<PageId, DbError> {
        if let Some(page_id) = self.free_pages.pop() {
            let buf = [0u8; PAGE_SIZE];
            self.file.write_page(page_id, &buf)?;
            if self.page_cache.contains(page_id) {
                self.restore_page(page_id, &buf)?;
                self.page_cache.mark_clean(page_id)?;
            }
            return Ok(page_id);
        }

        let page_id = self.page_count()? as PageId;

        let buf = [0u8; PAGE_SIZE];
        self.file.write_page(page_id, &buf)?;

        Ok(page_id)
    }

    /// Makes a newly allocated page available for reuse in this pager session.
    /// Persists the freed page ID to the persistent freelist.
    pub fn free_page(&mut self, page_id: PageId) -> Result<(), DbError> {
        if page_id == META_PAGE_ID || self.free_pages.contains(&page_id) {
            return Ok(());
        }

        let buf = [0u8; PAGE_SIZE];
        self.file.write_page(page_id, &buf)?;
        if self.page_cache.contains(page_id) {
            self.restore_page(page_id, &buf)?;
            self.page_cache.mark_clean(page_id)?;
        }
        self.free_pages.push(page_id);
        self.freelist_dirty = true;
        Ok(())
    }

    /// Writes one dirty cached page back to the main database file and marks it clean.
    pub fn flush_page_to_disk(&mut self, page_id: PageId) -> Result<(), DbError> {
        let data = *self
            .page_cache
            .get(page_id)
            .ok_or(DbError::ReadError)?
            .data();
        self.file.write_page(page_id, &data)?;
        self.page_cache.mark_clean(page_id)?;
        Ok(())
    }

    /// Persists the in-memory free_pages list to freelist pages on disk.
    /// Allocates new pages at the end of the file for freelist storage.
    fn persist_freelist(&mut self) -> Result<(), DbError> {
        // Split free_pages into chunks that fit in one freelist page
        let chunks: Vec<Vec<PageId>> = self
            .free_pages
            .chunks(FreelistPage::MAX_ENTRIES)
            .map(|c| c.to_vec())
            .collect();

        // Allocate freelist pages (raw, not through free_pages to avoid recursion)
        let mut freelist_page_ids: Vec<PageId> = Vec::with_capacity(chunks.len());
        for _ in &chunks {
            let page_id = self.allocate_page_raw()?;
            freelist_page_ids.push(page_id);
        }

        // Write freelist data to pages and link them together
        for i in 0..chunks.len() {
            let next = if i + 1 < freelist_page_ids.len() {
                freelist_page_ids[i + 1]
            } else {
                0
            };
            self.write_freelist_page_data(freelist_page_ids[i], next, &chunks[i])?;
        }

        // Update meta header freelist_head
        let new_head = if freelist_page_ids.is_empty() {
            0
        } else {
            freelist_page_ids[0]
        };

        // Update both in-memory and on-disk freelist_head
        self.freelist_head = new_head;
        self.update_meta_freelist_head()?;

        self.freelist_dirty = false;
        Ok(())
    }

    /// Writes freelist page data to disk (page type + next pointer + count + entries).
    fn write_freelist_page_data(
        &mut self,
        page_id: PageId,
        next_page: PageId,
        entries: &[PageId],
    ) -> Result<(), DbError> {
        let mut buf = [0u8; PAGE_SIZE];
        let flp = FreelistPage {
            next_page,
            entries: entries.to_vec(),
        };
        flp.to_bytes(&mut buf);
        self.file.write_page(page_id, &buf)?;
        Ok(())
    }

    /// Allocates a new page without going through the freelist (raw append to file).
    fn allocate_page_raw(&mut self) -> Result<PageId, DbError> {
        let page_id = self.page_count()? as PageId;
        let buf = [0u8; PAGE_SIZE];
        self.file.write_page(page_id, &buf)?;
        Ok(page_id)
    }

    /// Updates the freelist_head field in the meta header on disk.
    fn update_meta_freelist_head(&mut self) -> Result<(), DbError> {
        let mut meta_buf = [0u8; PAGE_SIZE];
        self.file.read_page(META_PAGE_ID, &mut meta_buf)?;
        let mut meta = layout::read_meta_header(&meta_buf);
        meta.freelist_head = self.freelist_head;
        layout::write_meta_header(&mut meta_buf, &meta);
        self.file.write_page(META_PAGE_ID, &meta_buf)?;
        // Also update in cache if present
        if self.page_cache.contains(META_PAGE_ID)
            && let Some(cached) = self.page_cache.get_mut(META_PAGE_ID)
        {
            layout::write_meta_header(cached.data_mut(), &meta);
        }
        Ok(())
    }

    /// Flushes all dirty cached pages to the database file, then flushes the file writer.
    pub fn flush_file(&mut self) -> Result<(), DbError> {
        let dirty_pages: Vec<PageId> = self.page_cache.dirty_page_ids().into_iter().collect();
        for page_id in dirty_pages {
            if self.page_cache.get(page_id).is_some() {
                self.flush_page_to_disk(page_id)?;
            }
        }
        self.file.flush()
    }

    /// Flushes dirty pages and asks the OS to sync the database file to storage.
    pub fn sync_file(&mut self) -> Result<(), DbError> {
        self.flush_file()?;
        self.file.sync()
    }

    /// Syncs all pager-managed state to durable storage, including the persistent freelist.
    pub fn sync_all(&mut self) -> Result<(), DbError> {
        if self.freelist_dirty {
            self.persist_freelist()?;
        }
        self.sync_file()
    }

    /// Flushes all pager-managed dirty pages without forcing an OS-level sync.
    /// Includes the persistent freelist if dirty.
    pub fn flush_all(&mut self) -> Result<(), DbError> {
        if self.freelist_dirty {
            self.persist_freelist()?;
        }
        self.flush_file()
    }

    /// Writes the provided page image directly to disk without updating the page cache.
    pub fn write_page_to_disk(
        &mut self,
        page_id: PageId,
        data: &[u8; PAGE_SIZE],
    ) -> Result<(), DbError> {
        self.file.write_page(page_id, data)
    }

    /// Reads one page directly from disk without consulting or updating the page cache.
    pub fn read_page_from_disk(&mut self, page_id: PageId) -> Result<[u8; PAGE_SIZE], DbError> {
        let mut buf = [0u8; PAGE_SIZE];
        self.file.read_page(page_id, &mut buf)?;
        Ok(buf)
    }
}
