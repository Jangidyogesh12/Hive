use crate::db::hive_db::HiveDb;
use crate::errors::DbError;
use crate::wal::wal_entry::TxId;

pub struct Transaction<'a> {
    db: &'a mut HiveDb,
    tx_id: TxId,
}

impl<'a> Transaction<'a> {
    pub(crate) fn new(db: &'a mut HiveDb, tx_id: TxId) -> Result<Self, DbError> {
        Ok(Self { db, tx_id })
    }

    /// Returns the transaction ID.
    pub fn tx_id(&self) -> TxId {
        self.tx_id
    }

    /// Commits the transaction by writing dirty page images to the WAL,
    /// syncing, and stamping page LSNs.
    pub fn commit(self) -> Result<(), DbError> {
        self.db.commit_tx(self.tx_id)
    }
}
