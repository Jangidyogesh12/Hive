//! Public Rust API for Hive.
//!
//! This crate is the stable Rust entrypoint for applications embedding Hive.
//! It currently re-exports `hive_core` directly while the higher-level public
//! API is still being polished for `v0.1.0`.
//!
//! # Example
//!
//! ```no_run
//! use hive::HiveDb;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut db = HiveDb::open(std::path::Path::new("./.hive"))?;
//! db.close();
//! # Ok(())
//! # }
//! ```

pub use hive_core::*;

pub use hive_core::db::hive_db::{Edge, HiveDb, HiveDbInfo, Node, Property};
pub use hive_core::errors::DbError;
pub use hive_core::query::executor::Executor;
pub use hive_core::query::parser::parse;
pub use hive_core::query::planner::plan;
pub use hive_core::query::result::QueryResult;
pub use hive_core::transaction::Transaction;
pub use hive_core::types::{EdgeId, NodeId, PropertyId};
pub use hive_core::value::Value;

/// Common imports for applications embedding Hive.
pub mod prelude {
    pub use hive_core::db::hive_db::{Edge, HiveDb, HiveDbInfo, Node, Property};
    pub use hive_core::errors::DbError;
    pub use hive_core::query::executor::Executor;
    pub use hive_core::query::parser::parse;
    pub use hive_core::query::planner::plan;
    pub use hive_core::query::result::QueryResult;
    pub use hive_core::types::{EdgeId, NodeId, PropertyId};
    pub use hive_core::value::Value;
}
