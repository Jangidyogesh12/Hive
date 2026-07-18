// Query executor — will be rebuilt on Pager-based storage.
use crate::errors::DbError;
use crate::query::result::QueryResult;

pub fn execute(
    _plan: &crate::query::planner::QueryPlan,
    _db: &mut crate::db::hive_db::HiveDb,
) -> Result<QueryResult, DbError> {
    Err(DbError::ReadError)
}
