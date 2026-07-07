use super::format::{META_HEADER_SIZE, PAGE_SIZE, REGULAR_HEADER_SIZE, SLOT_ENTRY_SIZE};
/// Page layout operations: init, insert, read, delete, compact.
///
/// All functions operate on a raw `&mut [u8; PAGE_SIZE]` buffer. The caller
/// (pager) owns the buffer. These functions are pure byte manipulation — no
/// file I/O, no allocation, no caching.
use super::format::{MetaHeader, PageHeader, PageType, SlotEntry};
use super::record::SlotIndex;
use super::serializer;
use crate::errors::DbError;

pub fn init_meta_page(buf: &mut [u8; PAGE_SIZE], meta: &MetaHeader) {
    buf.fill(0);
    meta.to_bytes(buf);
}

pub fn init_regular_page(buf: &mut [u8; PAGE_SIZE], page_type: PageType) {
    buf.fill(0);
    let header = PageHeader::new(page_type);
    header.to_bytes(buf);
}

pub fn read_page_header(buf: &[u8; PAGE_SIZE]) -> PageHeader {
    PageHeader::from_bytes(buf)
}

pub fn read_meta_header(buf: &[u8; PAGE_SIZE]) -> MetaHeader {
    MetaHeader::from_bytes(buf)
}

pub fn write_page_header(buf: &mut [u8; PAGE_SIZE], header: &PageHeader) {
    header.to_bytes(buf);
}

pub fn write_meta_header(buf: &mut [u8; PAGE_SIZE], meta: &MetaHeader) {
    meta.to_bytes(buf);
}

pub fn update_checksum(buf: &mut [u8; PAGE_SIZE]) {
    let cs = serializer::compute_checksum(buf, PageHeader::CHECKSUM_START, PAGE_SIZE);
    serializer::put_u32_le(buf, 8, cs);
}

pub fn verify_checksum(buf: &[u8; PAGE_SIZE]) -> bool {
    let stored = serializer::get_u32_le(buf, 8);
    let computed = serializer::compute_checksum(buf, PageHeader::CHECKSUM_START, PAGE_SIZE);
    stored == computed
}

pub fn slot_offset(slot_idx: u16, content_start: usize) -> usize {
    content_start + (slot_idx as usize) * SLOT_ENTRY_SIZE
}

fn content_start_for_page(buf: &[u8; PAGE_SIZE]) -> usize {
    let page_type = serializer::get_u8(buf, 0);
    if page_type == PageType::Meta as u8 {
        META_HEADER_SIZE
    } else {
        REGULAR_HEADER_SIZE
    }
}

pub fn get_free_space(buf: &[u8; PAGE_SIZE]) -> usize {
    let header = read_page_header(buf);
    let content_start = content_start_for_page(buf);
    let slot_end = content_start + (header.slot_count as usize) * SLOT_ENTRY_SIZE;
    let free_offset = header.free_space_offset as usize;
    free_offset.saturating_sub(slot_end)
}

pub fn insert_record(buf: &mut [u8; PAGE_SIZE], record_bytes: &[u8]) -> Result<SlotIndex, DbError> {
    let mut header = read_page_header(buf);
    let content_start = content_start_for_page(buf);
    let record_len = record_bytes.len();
    let required_space = record_len + SLOT_ENTRY_SIZE;
    let free = get_free_space(buf);

    if free < required_space {
        compact_page(buf)?;
        let free_after = get_free_space(buf);
        if free_after < required_space {
            return Err(DbError::WriteError);
        }
        header = read_page_header(buf);
    }

    let new_content_offset = header.free_space_offset as usize - record_len;
    buf[new_content_offset..new_content_offset + record_len].copy_from_slice(record_bytes);

    let slot_pos = slot_offset(header.slot_count, content_start);
    let slot = SlotEntry::new(new_content_offset as u16, record_len as u16);
    slot.to_bytes(&mut buf[slot_pos..slot_pos + SLOT_ENTRY_SIZE]);

    header.slot_count += 1;
    header.free_space_offset = new_content_offset as u16;
    write_page_header(buf, &header);
    update_checksum(buf);

    Ok(SlotIndex(header.slot_count - 1))
}

pub fn read_record_bytes(buf: &[u8; PAGE_SIZE], slot_idx: u16) -> Option<&[u8]> {
    let header = read_page_header(buf);
    if slot_idx >= header.slot_count {
        return None;
    }
    let content_start = content_start_for_page(buf);
    let slot_pos = slot_offset(slot_idx, content_start);
    let slot = SlotEntry::from_bytes(&buf[slot_pos..slot_pos + SLOT_ENTRY_SIZE]);
    if slot.is_dead() {
        return None;
    }
    let start = slot.offset as usize;
    let end = start + slot.length as usize;
    if end > PAGE_SIZE {
        return None;
    }
    Some(&buf[start..end])
}

pub fn read_record(buf: &[u8; PAGE_SIZE], slot_idx: u16) -> Option<Vec<u8>> {
    read_record_bytes(buf, slot_idx).map(|s| s.to_vec())
}

pub fn delete_record(buf: &mut [u8; PAGE_SIZE], slot_idx: u16) -> Result<(), DbError> {
    let header = read_page_header(buf);
    if slot_idx >= header.slot_count {
        return Err(DbError::ReadError);
    }
    let content_start = content_start_for_page(buf);
    let slot_pos = slot_offset(slot_idx, content_start);
    let slot = SlotEntry::from_bytes(&buf[slot_pos..slot_pos + SLOT_ENTRY_SIZE]);
    if slot.is_dead() {
        return Ok(());
    }
    add_to_freeblock(buf, slot.offset as usize, slot.length as usize)?;
    let dead = SlotEntry::new(SlotEntry::DEAD, 0);
    dead.to_bytes(&mut buf[slot_pos..slot_pos + SLOT_ENTRY_SIZE]);
    update_checksum(buf);
    Ok(())
}

fn add_to_freeblock(
    buf: &mut [u8; PAGE_SIZE],
    offset: usize,
    length: usize,
) -> Result<(), DbError> {
    if length < SLOT_ENTRY_SIZE {
        return Ok(());
    }
    let header = read_page_header(buf);
    if header.first_freeblock == 0 {
        let mut new_header = header;
        write_freeblock_header(buf, offset, length, 0);
        new_header.first_freeblock = offset as u16;
        write_page_header(buf, &new_header);
    } else {
        let mut current = header.first_freeblock as usize;
        loop {
            let next = serializer::get_u16_le(buf, current + 2) as usize;
            if next == 0 {
                write_freeblock_header(buf, current, 0, offset as u16);
                write_freeblock_header(buf, offset, length, 0);
                break;
            }
            current = next;
        }
    }
    update_checksum(buf);
    Ok(())
}

fn write_freeblock_header(buf: &mut [u8; PAGE_SIZE], offset: usize, length: usize, next: u16) {
    serializer::put_u16_le(buf, offset, length as u16);
    serializer::put_u16_le(buf, offset + 2, next);
}

pub fn compact_page(buf: &mut [u8; PAGE_SIZE]) -> Result<(), DbError> {
    let header = read_page_header(buf);
    let content_start = content_start_for_page(buf);
    let slot_count = header.slot_count as usize;
    if slot_count == 0 {
        return Ok(());
    }

    let mut records: Vec<(Vec<u8>, u16)> = Vec::with_capacity(slot_count);
    for i in 0..slot_count {
        let slot_pos = slot_offset(i as u16, content_start);
        let slot = SlotEntry::from_bytes(&buf[slot_pos..slot_pos + SLOT_ENTRY_SIZE]);
        if slot.is_dead() {
            continue;
        }
        let start = slot.offset as usize;
        let end = start + slot.length as usize;
        records.push((buf[start..end].to_vec(), i as u16));
    }

    if records.is_empty() {
        let mut new_header = PageHeader::new(header.page_type);
        new_header.lsn = header.lsn;
        write_page_header(buf, &new_header);
        update_checksum(buf);
        return Ok(());
    }

    let total_content: usize = records.iter().map(|(d, _)| d.len()).sum();
    let mut write_offset = PAGE_SIZE - total_content;

    for (data, _old_idx) in &records {
        buf[write_offset..write_offset + data.len()].copy_from_slice(data);
        write_offset += data.len();
    }

    let mut new_header = PageHeader::new(header.page_type);
    new_header.lsn = header.lsn;
    new_header.slot_count = records.len() as u16;
    new_header.free_space_offset = (PAGE_SIZE - total_content) as u16;
    write_page_header(buf, &new_header);

    let mut slot_offset_val = PAGE_SIZE - total_content;
    for (idx, (data, _old_idx)) in records.iter().enumerate() {
        let slot_pos = content_start + idx * SLOT_ENTRY_SIZE;
        let slot = SlotEntry::new(slot_offset_val as u16, data.len() as u16);
        slot.to_bytes(&mut buf[slot_pos..slot_pos + SLOT_ENTRY_SIZE]);
        slot_offset_val += data.len();
    }

    let zeros_start = content_start + records.len() * SLOT_ENTRY_SIZE;
    let zeros_end = new_header.free_space_offset as usize;
    if zeros_start < zeros_end {
        buf[zeros_start..zeros_end].fill(0);
    }

    update_checksum(buf);
    Ok(())
}

pub fn live_slot_count(buf: &[u8; PAGE_SIZE]) -> usize {
    let header = read_page_header(buf);
    let content_start = content_start_for_page(buf);
    let mut count = 0;
    for i in 0..header.slot_count {
        let slot_pos = slot_offset(i, content_start);
        let slot = SlotEntry::from_bytes(&buf[slot_pos..slot_pos + SLOT_ENTRY_SIZE]);
        if !slot.is_dead() {
            count += 1;
        }
    }
    count
}
