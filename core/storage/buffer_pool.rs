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
    pub fn new(capacity: usize) -> Self {
        let mut free = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            free.push(Box::new([0; PAGE_SIZE]));
        }
        Self { capacity, free }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn available(&self) -> usize {
        self.free.len()
    }

    pub fn used(&self) -> usize {
        self.capacity - self.available()
    }

    pub fn acquire(&mut self) -> Option<PageBuffer> {
        self.free.pop().map(|mut buffer| {
            buffer.fill(0);
            buffer
        })
    }

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
