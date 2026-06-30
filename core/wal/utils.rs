use crc32fast::Hasher;

pub(super) fn crc32_for_entry(entry_type: u8, payload: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(&[entry_type]);
    hasher.update(payload);
    hasher.finalize()
}
