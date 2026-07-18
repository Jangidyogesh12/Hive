use super::page::format::PAGE_SIZE;

pub type PageBuffer = Box<[u8; PAGE_SIZE]>;

/// Fixed-size arena for reusable page buffers.
///
/// The pool does not know page ids or eviction policy. It only hands out and
/// recycles zeroed 4KB buffers so higher layers avoid per-page allocations.
pub struct BufferPool {
    capacity: usize,
    free: Vec<PageBuffer>,
}

impl BufferPool {
    /// Creates a pool pre-filled with zeroed page-sized buffers.
    pub fn new(capacity: usize) -> Self {
        let mut free = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            free.push(Box::new([0; PAGE_SIZE]));
        }
        Self { capacity, free }
    }

    /// Returns the maximum number of page buffers this pool can hold.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns how many buffers are currently free for reuse.
    pub fn available(&self) -> usize {
        self.free.len()
    }

    /// Returns how many buffers are currently checked out by callers.
    pub fn used(&self) -> usize {
        self.capacity - self.available()
    }

    /// Takes one zeroed page buffer from the pool.
    pub fn acquire(&mut self) -> Option<PageBuffer> {
        self.free.pop().map(|mut buffer| {
            buffer.fill(0);
            buffer
        })
    }

    /// Returns a page buffer to the pool after clearing its old contents.
    pub fn release(&mut self, mut buffer: PageBuffer) {
        buffer.fill(0);
        self.free.push(buffer);
    }
}

impl Default for BufferPool {
    fn default() -> Self {
        Self::new(128)
    }
}
