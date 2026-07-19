use crate::errors::DbError;
use crate::storage::page::format::PAGE_SIZE;
use crate::storage::pager::{Lsn, Pager};
use crate::wal::Wal;
use crate::wal::wal_entry::{TxId, WalEntry};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub enum RecoveryOutcome {
    Clean,
    Recovered {
        committed_tx_count: usize,
        pages_redone: usize,
    },
}

pub fn recover(db_dir: &Path, pager: &mut Pager) -> Result<RecoveryOutcome, DbError> {
    let wal_path = db_dir.join("wal.hive");
    let mut wal = Wal::open(&wal_path)?;

    let entries = wal.read_all()?;

    if entries.is_empty() {
        return Ok(RecoveryOutcome::Clean);
    }

    let mut committed_txs: HashSet<TxId> = HashSet::new();
    let mut _last_checkpoint_lsn: Lsn = 0;
    let mut tx_start_lsn: HashMap<TxId, Lsn> = HashMap::new();
    let mut max_lsn: Lsn = 0;

    for entry in &entries {
        let lsn = entry.lsn();
        if lsn > max_lsn {
            max_lsn = lsn;
        }

        match entry {
            WalEntry::Begin { tx_id, lsn } => {
                tx_start_lsn.insert(*tx_id, *lsn);
            }
            WalEntry::PageImage { .. } => {}
            WalEntry::Commit { tx_id, .. } => {
                committed_txs.insert(*tx_id);
            }
            WalEntry::Checkpoint { lsn } => {
                _last_checkpoint_lsn = *lsn;
            }
        }
    }

    let mut pages_redone = 0;

    for entry in &entries {
        if let WalEntry::PageImage {
            tx_id,
            page_lsn,
            page_id,
            bytes,
            ..
        } = entry
        {
            if !committed_txs.contains(tx_id) {
                continue;
            }

            let disk_page = pager.read_page_from_disk(*page_id)?;
            let disk_page_lsn = extract_page_lsn(&disk_page);

            if *page_lsn > disk_page_lsn {
                pager.write_page_to_disk(*page_id, bytes)?;
                pages_redone += 1;
            }
        }
    }

    pager.set_next_lsn(max_lsn + 1);

    if pages_redone > 0 || !committed_txs.is_empty() {
        Ok(RecoveryOutcome::Recovered {
            committed_tx_count: committed_txs.len(),
            pages_redone,
        })
    } else {
        Ok(RecoveryOutcome::Clean)
    }
}

fn extract_page_lsn(page_bytes: &[u8; PAGE_SIZE]) -> Lsn {
    u32::from_le_bytes(page_bytes[12..16].try_into().unwrap()) as Lsn
}
