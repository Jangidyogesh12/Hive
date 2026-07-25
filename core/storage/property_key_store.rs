/// Property-key dictionary stored in dedicated PropertyKeyData pages.
///
/// Each entry is `[key_id: u32][name_len: u16][name: bytes]`.
/// The root property-key page ID is stored in MetaHeader.root_string_page.
use crate::errors::DbError;
use crate::storage::page::format::META_PAGE_ID;
use crate::storage::page::format::PageHeader;
use crate::storage::page::layout;
use crate::storage::pager::Pager;

/// Size of the fixed header before the name bytes in a property-key entry: `[key_id: u32][name_len: u16]`.
pub(crate) const PROPERTY_KEY_ENTRY_HEADER_SIZE: usize = 6;

/// In-memory handle for the property-key dictionary stored in `PropertyKeyData` pages.
///
/// The dictionary maps `key_id <-> name` and lives in the root page pointed to
/// by `MetaHeader.root_string_page`.  Every property entry on a node or edge
/// stores a `key_id` that references this dictionary.
pub struct PropertyKeyStore;

impl PropertyKeyStore {
    /// Encodes a single property-key dictionary entry as `[key_id: u32][name_len: u16][name: bytes]`.
    pub(crate) fn encode_property_key_entry(key_id: u32, name: &str) -> Result<Vec<u8>, DbError> {
        let name_bytes = name.as_bytes();
        if name_bytes.len() > u16::MAX as usize {
            return Err(DbError::WriteError);
        }

        let entry_size = PROPERTY_KEY_ENTRY_HEADER_SIZE + name_bytes.len();
        let mut entry_buf = vec![0u8; entry_size];
        entry_buf[0..4].copy_from_slice(&key_id.to_le_bytes());
        entry_buf[4..6].copy_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        entry_buf[6..].copy_from_slice(name_bytes);
        Ok(entry_buf)
    }

    /// Looks up the `key_id` for the given property name, or returns `None` if not found.
    pub fn find_property_key(pager: &mut Pager, name: &str) -> Result<Option<u32>, DbError> {
        let root_page = {
            let meta_page = pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.root_string_page
        };

        if root_page == 0 {
            return Ok(None);
        }

        let name_bytes = name.as_bytes();
        let page_buf = pager.get_page(root_page)?;
        let header = PageHeader::from_bytes(page_buf);

        for slot_idx in 0..header.slot_count {
            if let Some(entry_bytes) = layout::read_record_bytes(page_buf, slot_idx) {
                if entry_bytes.len() < PROPERTY_KEY_ENTRY_HEADER_SIZE {
                    continue;
                }
                let stored_len = u16::from_le_bytes(entry_bytes[4..6].try_into().unwrap()) as usize;
                if stored_len == name_bytes.len()
                    && entry_bytes[PROPERTY_KEY_ENTRY_HEADER_SIZE
                        ..PROPERTY_KEY_ENTRY_HEADER_SIZE + stored_len]
                        == *name_bytes
                {
                    let key_id = u32::from_le_bytes(entry_bytes[0..4].try_into().unwrap());
                    return Ok(Some(key_id));
                }
            }
        }

        Ok(None)
    }

    /// Returns the property name for the given `key_id`, or `None` if the key is unknown.
    pub fn get_property_key_name(
        pager: &mut Pager,
        key_id: u32,
    ) -> Result<Option<String>, DbError> {
        let root_page = {
            let meta_page = pager.get_page(META_PAGE_ID)?;
            let meta = layout::read_meta_header(meta_page);
            meta.root_string_page
        };

        if root_page == 0 {
            return Ok(None);
        }

        let page_buf = pager.get_page(root_page)?;
        let header = PageHeader::from_bytes(page_buf);

        for slot_idx in 0..header.slot_count {
            if let Some(entry_bytes) = layout::read_record_bytes(page_buf, slot_idx) {
                if entry_bytes.len() < PROPERTY_KEY_ENTRY_HEADER_SIZE {
                    continue;
                }
                let stored_id = u32::from_le_bytes(entry_bytes[0..4].try_into().unwrap());
                if stored_id == key_id {
                    let name_len =
                        u16::from_le_bytes(entry_bytes[4..6].try_into().unwrap()) as usize;
                    let name = String::from_utf8_lossy(
                        &entry_bytes[PROPERTY_KEY_ENTRY_HEADER_SIZE
                            ..PROPERTY_KEY_ENTRY_HEADER_SIZE + name_len],
                    )
                    .into_owned();
                    return Ok(Some(name));
                }
            }
        }

        Ok(None)
    }
}
