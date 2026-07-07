//! Core engine for Hive, a local-first graph database.
//!
//! `hive_core` contains the storage engine, query parser/planner/executor,
//! indexes, write-ahead log, recovery path, and transaction support. Most
//! applications should depend on the public `hive` crate, which re-exports this
//! crate during the pre-`v0.1.0` API stabilization period.
//!
//! # Query execution flow
//!
//! 1. Parse Cypher with [`query::parser::parse`].
//! 2. Convert the AST to a plan with [`query::planner::plan`].
//! 3. Execute the plan with [`query::executor::Executor`].
//!
//! # Direct storage API
//!
//! The [`db::hive_db::HiveDb`] type exposes lower-level graph operations such as
//! creating nodes and edges, setting properties, traversing neighbors, and
//! opening transactions.

pub mod db;
pub mod errors;
pub mod query;
pub mod storage;
pub mod store;
pub mod transaction;
pub mod types;
pub mod value;
pub mod wal;
