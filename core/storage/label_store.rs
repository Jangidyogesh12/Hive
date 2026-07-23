/// Label dictionary stored in dedicated LabelData pages.
///
/// Each label entry is `[label_id: u32][name_len: u16][name: bytes]`.
/// The root label page ID is stored in MetaHeader.root_label_page.
use crate::errors::DbError;
use crate::storage::page::format::META_PAGE_ID;
use crate::storage::page::format::PageHeader;
use crate::storage::page::layout;
use crate::storage::pager::Pager;

pub(crate) const LABEL_ENTRY_HEADER_SIZE: usize = 6; // label_id (4) + name_len (2)

pub struct LabelStore;

impl LabelStore {
    pub(crate) fn encode_label_entry(label_id: u32, name: &str) -> Result<Vec<u8>, DbError> {
        let name_bytes = name.as_bytes();
        if name_bytes.len() > u16::MAX as usize {
            return Err(DbError::WriteError);
        }

        let entry_size = LABEL_ENTRY_HEADER_SIZE + name_bytes.len();
        let mut entry_buf = vec![0u8; entry_size];
        entry_buf[0..4].copy_from_slice(&label_id.to_le_bytes());
        entry_buf[4..6].copy_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        entry_buf[6..].copy_from_slice(name_bytes);
        Ok(entry_buf)
    }

    /// Looks up a label name and returns its ID, or None if not found.
    pub fn find_label(pager: &mut Pager, name: &str) -> Result<Option<u32>, DbError> {
        let root_page = {
            let meta_page = pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.root_label_page
        };

        if root_page == 0 {
            return Ok(None);
        }

        let name_bytes = name.as_bytes();
        let page_buf = pager.get_page(root_page)?;
        let header = PageHeader::from_bytes(page_buf);

        for slot_idx in 0..header.slot_count {
            if let Some(entry_bytes) = layout::read_record_bytes(page_buf, slot_idx) {
                if entry_bytes.len() < LABEL_ENTRY_HEADER_SIZE {
                    continue;
                }
                let stored_len = u16::from_le_bytes(entry_bytes[4..6].try_into().unwrap()) as usize;
                if stored_len == name_bytes.len()
                    && entry_bytes[LABEL_ENTRY_HEADER_SIZE..LABEL_ENTRY_HEADER_SIZE + stored_len]
                        == *name_bytes
                {
                    let label_id = u32::from_le_bytes(entry_bytes[0..4].try_into().unwrap());
                    return Ok(Some(label_id));
                }
            }
        }

        Ok(None)
    }

    /// Returns the label name for a given ID, or None if not found.
    pub fn get_label_name(pager: &mut Pager, label_id: u32) -> Result<Option<String>, DbError> {
        let root_page = {
            let meta_page = pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.root_label_page
        };

        if root_page == 0 {
            return Ok(None);
        }

        let page_buf = pager.get_page(root_page)?;
        let header = PageHeader::from_bytes(page_buf);

        for slot_idx in 0..header.slot_count {
            if let Some(entry_bytes) = layout::read_record_bytes(page_buf, slot_idx) {
                if entry_bytes.len() < LABEL_ENTRY_HEADER_SIZE {
                    continue;
                }
                let stored_id = u32::from_le_bytes(entry_bytes[0..4].try_into().unwrap());
                if stored_id == label_id {
                    let name_len =
                        u16::from_le_bytes(entry_bytes[4..6].try_into().unwrap()) as usize;
                    let name = String::from_utf8_lossy(
                        &entry_bytes[LABEL_ENTRY_HEADER_SIZE..LABEL_ENTRY_HEADER_SIZE + name_len],
                    )
                    .into_owned();
                    return Ok(Some(name));
                }
            }
        }

        Ok(None)
    }
}
