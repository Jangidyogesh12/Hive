use crate::db::hive_db::HiveDb;
use crate::errors::DbError;

pub struct Transaction<'a> {
    #[allow(dead_code)]
    db: &'a mut HiveDb,
}

impl<'a> Transaction<'a> {
    pub fn new(db: &'a mut HiveDb) -> Result<Self, DbError> {
        Ok(Self { db })
    }

    pub fn commit(self) -> Result<(), DbError> {
        Ok(())
    }
}
