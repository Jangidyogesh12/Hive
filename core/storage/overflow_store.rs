/// Overflow page store for long strings that don't fit inline ( > 15 bytes).
///
/// Overflow page format:
/// - Regular page header (20 bytes)
/// - `[length: u32][data: bytes]` - the string length followed by raw bytes
///
/// `PropertyEntry.long_value_offset` stores the overflow page ID.
use crate::errors::DbError;
use crate::storage::page::format::{PageType, REGULAR_HEADER_SIZE};
use crate::storage::page::layout;
use crate::storage::pager::Pager;

pub struct OverflowStore;

impl OverflowStore {
    /// Writes a long string to a new overflow page and returns the page ID.
    pub fn write_string(pager: &mut Pager, data: &[u8]) -> Result<u32, DbError> {
        let page_id = pager.allocate_page()?;
        let page_buf = pager.get_page_mut(page_id)?;
        layout::init_regular_page(page_buf, PageType::Overflow);

        // Write length (4 bytes) + data after the regular header
        let offset = REGULAR_HEADER_SIZE;
        let len = data.len() as u32;
        page_buf[offset..offset + 4].copy_from_slice(&len.to_le_bytes());
        page_buf[offset + 4..offset + 4 + data.len()].copy_from_slice(data);

        Ok(page_id)
    }

    /// Reads a long string from an overflow page.
    pub fn read_string(pager: &mut Pager, page_id: u32) -> Result<Vec<u8>, DbError> {
        let page_buf = pager.get_page(page_id)?;
        let offset = REGULAR_HEADER_SIZE;

        let len = u32::from_le_bytes(
            page_buf[offset..offset + 4]
                .try_into()
                .map_err(|_| DbError::ReadError)?,
        ) as usize;

        Ok(page_buf[offset + 4..offset + 4 + len].to_vec())
    }
}
