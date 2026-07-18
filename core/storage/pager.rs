use super::buffer_pool::BufferPool;
use super::page::format::PAGE_SIZE;
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
    file: FileHandle,
    page_cache: PageCache,
    pool: BufferPool,
    next_lsn: AtomicU64,
}

impl Pager {
    /// Opens the pager for `hive.db` and creates its cache and reusable buffer pool.
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

        Ok(Self {
            file,
            page_cache,
            pool,
            next_lsn: AtomicU64::new(1),
        })
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
        let page_id = self.page_count()? as PageId;

        let buf = [0u8; PAGE_SIZE];
        self.file.write_page(page_id, &buf)?;

        Ok(page_id)
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

    /// Syncs all pager-managed state to durable storage.
    pub fn sync_all(&mut self) -> Result<(), DbError> {
        self.sync_file()
    }

    /// Flushes all pager-managed dirty pages without forcing an OS-level sync.
    pub fn flush_all(&mut self) -> Result<(), DbError> {
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
