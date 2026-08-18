//! B-tree page layout helpers.
//!
//! Hive B-tree pages reuse the existing 20-byte [`PageHeader`](super::format::PageHeader)
//! and interpret some fields differently:
//!
//! * `slot_count` is the **cell count**.
//! * `free_space_offset` is the start of the **cell content area**.
//! * `first_freeblock` is the offset of the first freeblock.
//! * `reserved` holds the **leftmost child pointer** for interior pages.
//!
//! After the header comes a cell pointer array (two bytes per cell), followed by
//! free space, followed by the cell content area which grows downward from the
//! end of the page.

use super::key::cmp_encoded;
use crate::errors::DbError;
use crate::storage::page::format::{PAGE_SIZE, PageType};
use crate::storage::page::layout::update_checksum;
use crate::storage::page::serializer;

pub const BTREE_HEADER_SIZE: usize = 20;
pub const CELL_POINTER_SIZE: usize = 2;
/// Smallest cell size we are willing to track as a freeblock.
pub const MIN_CELL_SIZE: usize = 4;

/// Returns the byte offset where the cell pointer array starts.
#[inline]
pub fn cell_pointer_array_start() -> usize {
    BTREE_HEADER_SIZE
}

/// Initializes a page as an empty B-tree leaf page.
pub fn init_leaf_page(buf: &mut [u8; PAGE_SIZE]) {
    buf.fill(0);
    let mut header = crate::storage::page::format::PageHeader::new(PageType::IndexLeaf);
    header.free_space_offset = PAGE_SIZE as u16;
    header.to_bytes(buf);
    update_checksum(buf);
}

/// Initializes a page as an empty B-tree interior page.
pub fn init_interior_page(buf: &mut [u8; PAGE_SIZE]) {
    buf.fill(0);
    let mut header = crate::storage::page::format::PageHeader::new(PageType::IndexInterior);
    header.free_space_offset = PAGE_SIZE as u16;
    header.to_bytes(buf);
    update_checksum(buf);
}

/// Returns true if the page is a B-tree leaf page.
pub fn is_leaf(buf: &[u8; PAGE_SIZE]) -> bool {
    matches!(page_type(buf), PageType::IndexLeaf)
}

/// Returns true if the page is a B-tree interior page.
pub fn is_interior(buf: &[u8; PAGE_SIZE]) -> bool {
    matches!(page_type(buf), PageType::IndexInterior)
}

fn page_type(buf: &[u8; PAGE_SIZE]) -> PageType {
    PageType::from_u8(serializer::get_u8(buf, 0)).unwrap_or(PageType::IndexLeaf)
}

pub fn cell_count(buf: &[u8; PAGE_SIZE]) -> usize {
    serializer::get_u16_le(buf, 2) as usize
}

fn set_cell_count(buf: &mut [u8; PAGE_SIZE], count: usize) {
    serializer::put_u16_le(buf, 2, count as u16);
}

pub fn cell_content_area(buf: &[u8; PAGE_SIZE]) -> usize {
    let v = serializer::get_u16_le(buf, 4) as usize;
    if v == 0 { PAGE_SIZE } else { v }
}

fn set_cell_content_area(buf: &mut [u8; PAGE_SIZE], offset: usize) {
    serializer::put_u16_le(buf, 4, offset as u16);
}

pub fn first_freeblock(buf: &[u8; PAGE_SIZE]) -> u16 {
    serializer::get_u16_le(buf, 6)
}

fn set_first_freeblock(buf: &mut [u8; PAGE_SIZE], offset: u16) {
    serializer::put_u16_le(buf, 6, offset);
}

pub fn fragmented_bytes(buf: &[u8; PAGE_SIZE]) -> usize {
    serializer::get_u8(buf, 7) as usize
}

fn set_fragmented_bytes(buf: &mut [u8; PAGE_SIZE], count: usize) {
    serializer::put_u8(buf, 7, count as u8);
}

/// Leftmost child pointer for an interior page (stored in the reserved field).
///
/// In an interior page, the leftmost pointer covers keys less than the first
/// cell's key.  Cell `i` stores the right child covering `[key_i, key_{i+1})`.
pub fn leftmost_pointer(buf: &[u8; PAGE_SIZE]) -> u32 {
    serializer::get_u32_le(buf, 16)
}

pub(crate) fn set_leftmost_pointer(buf: &mut [u8; PAGE_SIZE], page_id: u32) {
    serializer::put_u32_le(buf, 16, page_id);
}

/// Byte offset of the cell pointer for `cell_idx`.
fn cell_pointer_offset(cell_idx: usize) -> usize {
    cell_pointer_array_start() + cell_idx * CELL_POINTER_SIZE
}

pub fn read_cell_pointer(buf: &[u8; PAGE_SIZE], cell_idx: usize) -> u16 {
    serializer::get_u16_le(buf, cell_pointer_offset(cell_idx))
}

fn write_cell_pointer(buf: &mut [u8; PAGE_SIZE], cell_idx: usize, offset: u16) {
    serializer::put_u16_le(buf, cell_pointer_offset(cell_idx), offset);
}

/// Returns the cell payload bytes for `cell_idx`, if the pointer is in range.
pub fn cell_bytes(buf: &[u8; PAGE_SIZE], cell_idx: usize) -> Option<&[u8]> {
    if cell_idx >= cell_count(buf) {
        return None;
    }
    let offset = read_cell_pointer(buf, cell_idx) as usize;
    if offset == 0 || offset > PAGE_SIZE {
        return None;
    }
    // Cell layout: [key_len: u16][key bytes][payload]
    let key_len = serializer::get_u16_le(buf, offset) as usize;
    let payload_len = cell_payload_size(buf, cell_idx)?;
    let end = offset + 2 + key_len + payload_len;
    if end > PAGE_SIZE {
        return None;
    }
    Some(&buf[offset..end])
}

/// Returns the key bytes for `cell_idx`.
pub fn cell_key_bytes(buf: &[u8; PAGE_SIZE], cell_idx: usize) -> Option<&[u8]> {
    let offset = read_cell_pointer(buf, cell_idx) as usize;
    let key_len = serializer::get_u16_le(buf, offset) as usize;
    Some(&buf[offset + 2..offset + 2 + key_len])
}

/// Returns the size of the payload following the encoded key for `cell_idx`.
fn cell_payload_size(buf: &[u8; PAGE_SIZE], cell_idx: usize) -> Option<usize> {
    let offset = read_cell_pointer(buf, cell_idx) as usize;
    let key_len = serializer::get_u16_le(buf, offset) as usize;
    let payload_start = offset + 2 + key_len;
    if is_leaf(buf) {
        // [rid_count: u16] [rid: u64] * count
        let count = serializer::get_u16_le(buf, payload_start) as usize;
        Some(2 + count * 8)
    } else {
        // [left_child_page: u32]
        Some(4)
    }
}

/// Total bytes used by the cell including key length field and payload.
pub fn cell_size(buf: &[u8; PAGE_SIZE], cell_idx: usize) -> Option<usize> {
    let offset = read_cell_pointer(buf, cell_idx) as usize;
    let key_len = serializer::get_u16_le(buf, offset) as usize;
    cell_payload_size(buf, cell_idx).map(|payload| 2 + key_len + payload)
}

/// Free bytes available for new cells (including pointer overhead).
pub fn free_space(buf: &[u8; PAGE_SIZE]) -> usize {
    let pointer_end = cell_pointer_array_start() + cell_count(buf) * CELL_POINTER_SIZE;
    cell_content_area(buf).saturating_sub(pointer_end)
}

/// Binary search for a key. Returns `Ok(idx)` on exact match, `Err(idx)` for the
/// insertion point (the first cell greater than the key).
pub fn find_cell_position(buf: &[u8; PAGE_SIZE], key: &[u8]) -> Result<usize, usize> {
    let mut low = 0;
    let mut high = cell_count(buf);
    while low < high {
        let mid = (low + high) / 2;
        match cmp_encoded(cell_key_bytes(buf, mid).unwrap_or(&[]), key) {
            std::cmp::Ordering::Less => low = mid + 1,
            std::cmp::Ordering::Equal => return Ok(mid),
            std::cmp::Ordering::Greater => high = mid,
        }
    }
    Err(low)
}

/// Shifts the cell pointer array to make room for `count` new pointers at `idx`.
/// Does not write the new pointers or update the cell count.
pub fn shift_pointers_right(buf: &mut [u8; PAGE_SIZE], idx: usize, count: usize) {
    let current = cell_count(buf);
    if idx >= current {
        return;
    }
    let start = cell_pointer_array_start();
    let src = start + idx * CELL_POINTER_SIZE;
    let dst = src + count * CELL_POINTER_SIZE;
    let len = (current - idx) * CELL_POINTER_SIZE;
    buf.copy_within(src..src + len, dst);
}

/// Shifts the cell pointer array left to remove `count` pointers at `idx`.
pub fn shift_pointers_left(buf: &mut [u8; PAGE_SIZE], idx: usize, count: usize) {
    let current = cell_count(buf);
    if idx + count >= current {
        return;
    }
    let start = cell_pointer_array_start();
    let dst = start + idx * CELL_POINTER_SIZE;
    let src = start + (idx + count) * CELL_POINTER_SIZE;
    let len = (current - idx - count) * CELL_POINTER_SIZE;
    buf.copy_within(src..src + len, dst);
}

/// Inserts a cell at position `idx`, shifting the cell pointer array to make
/// room. The caller must ensure there is enough free space.
pub fn insert_cell(
    buf: &mut [u8; PAGE_SIZE],
    idx: usize,
    key: &[u8],
    payload: &[u8],
) -> Result<(), DbError> {
    let cell_size = 2 + key.len() + payload.len();
    let alloc_size = cell_size.max(MIN_CELL_SIZE);

    // Make room in the pointer array before allocating cell content, because
    // shifting the array consumes free space.
    shift_pointers_right(buf, idx, 1);

    let offset = allocate_cell_space(buf, alloc_size)?;

    // Write [key_len][key][payload]
    serializer::put_u16_le(buf, offset, key.len() as u16);
    buf[offset + 2..offset + 2 + key.len()].copy_from_slice(key);
    buf[offset + 2 + key.len()..offset + 2 + key.len() + payload.len()].copy_from_slice(payload);

    // If we over-allocated because of MIN_CELL_SIZE, the trailing bytes are
    // left as part of the cell; they are unreachable and become free when the
    // cell is deleted.

    write_cell_pointer(buf, idx, offset as u16);
    set_cell_count(buf, cell_count(buf) + 1);
    update_checksum(buf);
    Ok(())
}

/// Extracts all cells from a page as `(key, payload)` pairs.
pub fn extract_cells(buf: &[u8; PAGE_SIZE]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::with_capacity(cell_count(buf));
    for i in 0..cell_count(buf) {
        if let Some(cell) = cell_bytes(buf, i) {
            let key_len = serializer::get_u16_le(cell, 0) as usize;
            let key = cell[2..2 + key_len].to_vec();
            let payload = cell[2 + key_len..].to_vec();
            out.push((key, payload));
        }
    }
    out
}

/// Rebuilds a page from the supplied cells, preserving interior/leftmost pointer
/// for interior pages if `leftmost` is provided.
pub fn rebuild_page(
    buf: &mut [u8; PAGE_SIZE],
    page_type: PageType,
    cells: &[(Vec<u8>, Vec<u8>)],
    leftmost: Option<u32>,
) -> Result<(), DbError> {
    buf.fill(0);
    let mut header = crate::storage::page::format::PageHeader::new(page_type);
    header.free_space_offset = PAGE_SIZE as u16;
    header.to_bytes(buf);
    if let Some(r) = leftmost {
        set_leftmost_pointer(buf, r);
    }
    for (i, (key, payload)) in cells.iter().enumerate() {
        insert_cell(buf, i, key, payload)?;
    }
    update_checksum(buf);
    Ok(())
}

/// Removes a cell and reclaims its space as a freeblock.
pub fn delete_cell(buf: &mut [u8; PAGE_SIZE], idx: usize) -> Result<(), DbError> {
    if idx >= cell_count(buf) {
        return Err(DbError::ReadError);
    }
    let offset = read_cell_pointer(buf, idx) as usize;
    let size = cell_size(buf, idx)
        .unwrap_or(MIN_CELL_SIZE)
        .max(MIN_CELL_SIZE);
    shift_pointers_left(buf, idx, 1);
    set_cell_count(buf, cell_count(buf) - 1);
    free_cell_range(buf, offset, size)?;
    update_checksum(buf);
    Ok(())
}

/// Allocates `size` bytes from the page for a new cell.
///
/// First tries to reuse a large enough freeblock, otherwise grows the cell
/// content area downward.
fn allocate_cell_space(buf: &mut [u8; PAGE_SIZE], size: usize) -> Result<usize, DbError> {
    if size < MIN_CELL_SIZE {
        return Err(DbError::WriteError);
    }

    if let Some(offset) = find_free_slot(buf, size) {
        return Ok(offset);
    }

    let content_start = cell_content_area(buf);
    let pointer_end = cell_pointer_array_start() + (cell_count(buf) + 1) * CELL_POINTER_SIZE;
    if content_start < size || content_start - size < pointer_end {
        return Err(DbError::WriteError);
    }
    let new_offset = content_start - size;
    set_cell_content_area(buf, new_offset);
    Ok(new_offset)
}

/// Searches the freeblock chain for a contiguous block of at least `size` bytes,
/// updating the chain if a slot is consumed or shrunk.
fn find_free_slot(buf: &mut [u8; PAGE_SIZE], size: usize) -> Option<usize> {
    let mut prev = None;
    let mut cur = match first_freeblock(buf) {
        0 => return None,
        n => n as usize,
    };

    while cur + MIN_CELL_SIZE <= PAGE_SIZE {
        let (next, block_size) = read_freeblock(buf, cur);
        if block_size as usize >= size {
            let remaining = block_size as usize - size;
            if remaining < MIN_CELL_SIZE {
                // Use the whole block; remove it from the chain.
                if let Some(p) = prev {
                    write_freeblock_next(buf, p as u16, next);
                } else {
                    set_first_freeblock(buf, next);
                }
                let frag = fragmented_bytes(buf) + remaining;
                set_fragmented_bytes(buf, frag);
                return Some(cur);
            }
            // Shrink the block from the end.
            let new_offset = cur + remaining;
            write_freeblock_size(buf, cur, remaining as u16);
            return Some(new_offset);
        }
        prev = Some(cur);
        if next == 0 {
            return None;
        }
        cur = next as usize;
    }
    None
}

/// Adds a freed cell range to the freeblock chain, coalescing with neighbors.
fn free_cell_range(
    buf: &mut [u8; PAGE_SIZE],
    mut offset: usize,
    mut len: usize,
) -> Result<(), DbError> {
    if len < MIN_CELL_SIZE {
        set_fragmented_bytes(buf, fragmented_bytes(buf) + len);
        return Ok(());
    }
    if offset + len > PAGE_SIZE {
        return Err(DbError::WriteError);
    }

    let content_area = cell_content_area(buf);
    if offset + len == content_area {
        // Freed range is directly above the unallocated region: just grow it.
        set_cell_content_area(buf, offset);
        return Ok(());
    }

    let mut prev = None;
    let mut next = match first_freeblock(buf) {
        0 => None,
        n => Some(n as usize),
    };

    // Find the surrounding freeblocks in ascending offset order.
    while let Some(n) = next {
        if n >= offset {
            break;
        }
        prev = next;
        next = match read_freeblock(buf, n).0 {
            0 => None,
            v => Some(v as usize),
        };
    }

    // Merge with the next block if contiguous (allowing 1-3 byte gaps as
    // fragmentation that is now reclaimed).
    if let Some(n) = next
        && offset + len <= n
        && n - (offset + len) <= 3
    {
        let (_, next_size) = read_freeblock(buf, n);
        let gap = n - (offset + len);
        len += gap + next_size as usize;
        next = match read_freeblock(buf, n).0 {
            0 => None,
            v => Some(v as usize),
        };
    }

    // Merge with the previous block if contiguous.
    if let Some(p) = prev {
        let (_, prev_size) = read_freeblock(buf, p);
        let prev_end = p + prev_size as usize;
        if prev_end <= offset && offset - prev_end <= 3 {
            let gap = offset - prev_end;
            offset = p;
            len += gap + prev_size as usize;
            prev = None; // first block will be rewritten
        }
    }

    // Write the (possibly merged) block header.
    let next_ptr = next.map(|n| n as u16).unwrap_or(0);
    write_freeblock_header(buf, offset, len, next_ptr);

    if let Some(p) = prev {
        write_freeblock_next(buf, p as u16, offset as u16);
    } else {
        set_first_freeblock(buf, offset as u16);
    }

    Ok(())
}

fn read_freeblock(buf: &[u8; PAGE_SIZE], offset: usize) -> (u16, u16) {
    (
        serializer::get_u16_le(buf, offset),
        serializer::get_u16_le(buf, offset + 2),
    )
}

fn write_freeblock_header(buf: &mut [u8; PAGE_SIZE], offset: usize, size: usize, next: u16) {
    serializer::put_u16_le(buf, offset, next);
    serializer::put_u16_le(buf, offset + 2, size as u16);
}

fn write_freeblock_next(buf: &mut [u8; PAGE_SIZE], offset: u16, next: u16) {
    serializer::put_u16_le(buf, offset as usize, next);
}

fn write_freeblock_size(buf: &mut [u8; PAGE_SIZE], offset: usize, size: u16) {
    serializer::put_u16_le(buf, offset + 2, size);
}
