//! Shared database handle with snapshot-isolated concurrent reads.
//!
//! `SharedDb` pairs an `Arc<Mutex<HiveDb>>` primary handle with an admission
//! `RwLock`: writers take the admission lock exclusively and run against the
//! primary handle, while each read-only query opens a private read-only
//! snapshot (`HiveDb::open_snapshot`) under a shared admission guard. The
//! snapshot replays committed WAL entries into its own cache and never
//! writes the shared files, so readers run truly concurrently with each
//! other (snapshot isolation: a read observes committed state as of open)
//! while writers stay exclusive.
//!
//! Lock ordering rule: acquire the admission lock first, then the inner
//! mutex, once per operation; never hold either across a nested `SharedDb`
//! call. Poisoned locks surface as `DbError::QueryError` so applications
//! can fail fast instead of hanging.
//!
//! # Example
//!
//! ```no_run
//! use hive_core::db::shared::SharedDb;
//! use std::path::Path;
//!
//! let db = SharedDb::open(Path::new("./.hive-shared")).unwrap();
//! db.execute("CREATE (n:Person {name: \"Alice\"})").unwrap();
//! let result = db.execute("MATCH (n:Person) RETURN n.name AS name").unwrap();
//! println!("{result}");
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use crate::db::hive_db::{DbStats, HiveDb, QueryMetrics};
use crate::errors::DbError;
use crate::query::result::QueryResult;
use crate::value::Value;

fn poisoned(context: &str) -> DbError {
    DbError::QueryError(format!("shared database lock poisoned during {context}"))
}

/// A thread-safe, reference-counted handle to a `HiveDb`.
///
/// Cloning shares the same underlying database and admission lock. Writers
/// serialize through the inner mutex under an exclusive admission guard;
/// readers each open a private read-only snapshot under a shared guard, so
/// concurrent readers never block each other while writers stay exclusive.
/// Crash recovery and checkpointing run inside the write path, so they
/// remain correct under concurrency. A pooled snapshot cache (to avoid
/// per-query open cost) is future work.
#[derive(Clone)]
pub struct SharedDb {
    inner: Arc<Mutex<HiveDb>>,
    /// Admission control: writers hold `write()`, snapshot readers hold `read()`.
    admission: Arc<RwLock<()>>,
    /// Database directory, needed to open per-query read snapshots.
    /// `None` when built via `from_db` (snapshot reads then fail loudly).
    db_dir: Option<PathBuf>,
}

impl SharedDb {
    /// Opens (or creates) a database directory and wraps it in a shared handle.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        Ok(Self {
            inner: Arc::new(Mutex::new(HiveDb::open(path)?)),
            admission: Arc::new(RwLock::new(())),
            db_dir: Some(path.to_path_buf()),
        })
    }

    /// Wraps an already-open `HiveDb` (which must be closed/quiescent).
    ///
    /// Snapshot reads (`execute_read`) are unavailable on such handles
    /// because the database directory is unknown; they return a query
    /// error. Use `open` when concurrent reads are needed.
    pub fn from_db(db: HiveDb) -> Self {
        Self {
            inner: Arc::new(Mutex::new(db)),
            admission: Arc::new(RwLock::new(())),
            db_dir: None,
        }
    }

    /// Parses, plans, and executes a query under the exclusive admission lock.
    ///
    /// Queries take the exclusive lock because planning may register labels
    /// and read-only detection happens inside `HiveDb::execute`. Use
    /// [`SharedDb::execute_read`] when the query is known to be read-only and
    /// concurrent reads are desired.
    pub fn execute(&self, query: &str) -> Result<QueryResult, DbError> {
        let _admission = self.admission.write().map_err(|_| poisoned("execute"))?;
        let mut db = self.inner.lock().map_err(|_| poisoned("execute"))?;
        db.execute(query)
    }

    /// Executes a query with `$name` parameter bindings under the exclusive lock.
    pub fn execute_with_params(
        &self,
        query: &str,
        params: &HashMap<String, Value>,
    ) -> Result<QueryResult, DbError> {
        let _admission = self.admission.write().map_err(|_| poisoned("execute"))?;
        let mut db = self.inner.lock().map_err(|_| poisoned("execute"))?;
        db.execute_with_params(query, params)
    }

    /// Executes a query under the exclusive lock with per-stage timing.
    pub fn execute_timed(&self, query: &str) -> Result<(QueryResult, QueryMetrics), DbError> {
        let _admission = self.admission.write().map_err(|_| poisoned("execute"))?;
        let mut db = self.inner.lock().map_err(|_| poisoned("execute"))?;
        db.execute_timed(query)
    }

    /// Executes a parameterized query under the exclusive lock with per-stage timing.
    pub fn execute_with_params_timed(
        &self,
        query: &str,
        params: &HashMap<String, Value>,
    ) -> Result<(QueryResult, QueryMetrics), DbError> {
        let _admission = self.admission.write().map_err(|_| poisoned("execute"))?;
        let mut db = self.inner.lock().map_err(|_| poisoned("execute"))?;
        db.execute_with_params_timed(query, params)
    }

    /// Executes a known read-only query against a private read-only snapshot.
    ///
    /// Returns a `QueryError` if the plan is not read-only, so writers cannot
    /// sneak mutations through the shared-read path. The snapshot is opened
    /// under a shared admission guard (writers excluded, other readers
    /// allowed), replays committed WAL entries into its own cache, and never
    /// writes the shared files — so any number of readers run concurrently,
    /// each observing committed state as of its own open. Handles built via
    /// `from_db` have no directory and fail loudly here.
    pub fn execute_read(&self, query: &str) -> Result<QueryResult, DbError> {
        let statement = crate::query::parser::parse(query)
            .map_err(|err| DbError::QueryError(err.to_string()))?;
        let plan = crate::query::planner::plan(statement)?;
        if !plan.is_read_only() {
            return Err(DbError::QueryError(
                "execute_read requires a read-only query".to_string(),
            ));
        }
        let _admission = self.admission.read().map_err(|_| poisoned("read"))?;
        let dir = self.db_dir.clone().ok_or_else(|| {
            DbError::QueryError(
                "execute_read requires SharedDb::open (unknown database directory)".to_string(),
            )
        })?;
        let mut snapshot = HiveDb::open_snapshot(&dir)?;
        let result =
            crate::query::executor::execute_with_params(&plan, &mut snapshot, &HashMap::new());
        snapshot.close();
        result
    }

    /// Returns database statistics.
    pub fn stats(&self) -> Result<DbStats, DbError> {
        let _admission = self.admission.write().map_err(|_| poisoned("stats"))?;
        let mut db = self.inner.lock().map_err(|_| poisoned("stats"))?;
        db.stats()
    }

    /// Runs the storage integrity checker.
    pub fn check_integrity(&self) -> Result<Vec<String>, DbError> {
        let _admission = self.admission.write().map_err(|_| poisoned("check"))?;
        let mut db = self.inner.lock().map_err(|_| poisoned("check"))?;
        db.check_integrity()
    }

    /// Runs the index consistency checker.
    pub fn check_index_consistency(&self) -> Result<Vec<String>, DbError> {
        let _admission = self.admission.write().map_err(|_| poisoned("check"))?;
        let mut db = self.inner.lock().map_err(|_| poisoned("check"))?;
        db.check_index_consistency()
    }

    /// Creates a node-label index.
    pub fn create_node_label_index(&self, label: &str) -> Result<u32, DbError> {
        let _admission = self.admission.write().map_err(|_| poisoned("index"))?;
        let mut db = self.inner.lock().map_err(|_| poisoned("index"))?;
        db.create_node_label_index(label)
    }

    /// Creates a unique constraint on `(label, property_key)`.
    pub fn create_unique_constraint(&self, label: &str, key: &str) -> Result<(), DbError> {
        let _admission = self.admission.write().map_err(|_| poisoned("constraint"))?;
        let mut db = self.inner.lock().map_err(|_| poisoned("constraint"))?;
        db.create_unique_constraint(label, key)
    }

    /// Runs a closure with exclusive (`write`) access to the database.
    ///
    /// Prefer the typed helpers above when possible; this escape hatch exists
    /// for multi-statement atomic batches that must hold the lock across
    /// statements.
    pub fn with_write<T>(
        &self,
        f: impl FnOnce(&mut HiveDb) -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        let _admission = self.admission.write().map_err(|_| poisoned("with_write"))?;
        let mut db = self.inner.lock().map_err(|_| poisoned("with_write"))?;
        f(&mut db)
    }

    /// Runs a closure with shared (`read`) access to the database.
    pub fn with_read<T>(
        &self,
        f: impl FnOnce(&HiveDb) -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        // Shared admission (other readers allowed), then the inner mutex,
        // which the engine requires for all access.
        let _admission = self.admission.read().map_err(|_| poisoned("with_read"))?;
        let db = self.inner.lock().map_err(|_| poisoned("with_read"))?;
        f(&db)
    }
}
