use super::super::utils::utils::{cleanup_dir, temp_dir};
use crate::storage::page::format::PAGE_SIZE;
use crate::wal::{Wal, WalEntry};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

#[test]
fn wal_roundtrips_physical_entry_types() {
    let dir = temp_dir("wal_roundtrips_physical_entry_types");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    let mut page_data = [0u8; PAGE_SIZE];
    page_data[0] = 0x01;
    page_data[1] = 0x02;

    let entries = vec![
        WalEntry::Begin { tx_id: 1, lsn: 1 },
        WalEntry::PageImage {
            tx_id: 1,
            lsn: 2,
            page_id: 0,
            page_lsn: 2,
            bytes: Box::new(page_data),
        },
        WalEntry::Commit { tx_id: 1, lsn: 3 },
        WalEntry::Checkpoint { lsn: 4 },
    ];

    for entry in &entries {
        wal.append(entry).unwrap();
    }

    let got = wal.read_all().unwrap();
    assert_eq!(got.len(), entries.len());

    assert!(matches!(got[0], WalEntry::Begin { tx_id: 1, lsn: 1 }));
    match &got[1] {
        WalEntry::PageImage {
            tx_id,
            lsn,
            page_id,
            page_lsn,
            bytes,
        } => {
            assert_eq!(*tx_id, 1);
            assert_eq!(*lsn, 2);
            assert_eq!(*page_id, 0);
            assert_eq!(*page_lsn, 2);
            assert_eq!(bytes[0], 0x01);
            assert_eq!(bytes[1], 0x02);
        }
        _ => panic!("Expected PageImage"),
    }
    assert!(matches!(got[2], WalEntry::Commit { tx_id: 1, lsn: 3 }));
    assert!(matches!(got[3], WalEntry::Checkpoint { lsn: 4 }));

    cleanup_dir(&dir);
}

#[test]
fn wal_truncate_clears_entries() {
    let dir = temp_dir("wal_truncate_clears_entries");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    wal.append(&WalEntry::Begin { tx_id: 1, lsn: 1 }).unwrap();
    wal.truncate().unwrap();

    let got = wal.read_all().unwrap();
    assert!(got.is_empty());

    cleanup_dir(&dir);
}

#[test]
fn wal_bad_checksum_ignores_corrupt_tail() {
    let dir = temp_dir("wal_bad_checksum_ignores_corrupt_tail");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    let first = WalEntry::Begin { tx_id: 1, lsn: 1 };
    let second = WalEntry::Commit { tx_id: 1, lsn: 2 };

    wal.append(&first).unwrap();
    wal.append(&second).unwrap();

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[0xFF]).unwrap();
    file.flush().unwrap();

    let got = wal.read_all().unwrap();
    assert_eq!(got.len(), 1);
    assert!(matches!(got[0], WalEntry::Begin { tx_id: 1, lsn: 1 }));

    cleanup_dir(&dir);
}

#[test]
fn wal_partial_last_entry_is_ignored() {
    let dir = temp_dir("wal_partial_last_entry_is_ignored");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    let first = WalEntry::Checkpoint { lsn: 1 };
    wal.append(&first).unwrap();

    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(&10u32.to_le_bytes()).unwrap();
    file.write_all(&[1, 2, 3]).unwrap();
    file.flush().unwrap();

    let got = wal.read_all().unwrap();
    assert_eq!(got.len(), 1);
    assert!(matches!(got[0], WalEntry::Checkpoint { lsn: 1 }));

    cleanup_dir(&dir);
}

#[test]
fn wal_sync_succeeds_after_append() {
    let dir = temp_dir("wal_sync_succeeds_after_append");
    let path = dir.join("wal.hive");
    let mut wal = Wal::open(&path).unwrap();

    wal.append(&WalEntry::Begin { tx_id: 1, lsn: 1 }).unwrap();

    wal.sync().unwrap();

    cleanup_dir(&dir);
}
