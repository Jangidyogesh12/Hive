use crate::errors::DbError;
use crate::storage::pager::Pager;
use crate::wal::Wal;
use crate::wal::recovery::{RecoveryManager, RecoveryOutcome};
use std::{fs, path::Path};

pub struct HiveDb {
    pub(crate) pager: Pager,
    #[allow(dead_code)]
    pub(crate) wal: Wal,
}

impl HiveDb {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        fs::create_dir_all(path).map_err(|_| DbError::FileOpenError)?;

        let wal_path = path.join("wal.hive");
        let mut pager = Pager::open(path, 128, 128)?;
        let wal = Wal::open(&wal_path)?;

        let recovery_outcome = RecoveryManager::recover(path, &mut pager)?;

        match recovery_outcome {
            RecoveryOutcome::Clean => {}
            RecoveryOutcome::Recovered {
                committed_tx_count,
                pages_redone,
            } => {
                eprintln!(
                    "Recovery: {} transactions replayed, {} pages redone",
                    committed_tx_count, pages_redone
                );
            }
        }

        Ok(Self { pager, wal })
    }

    pub fn close(mut self) {
        let _ = self.pager.sync_all();
    }
}
