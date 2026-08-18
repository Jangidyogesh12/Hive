// Index store — re-exports the durable B+ tree implementation and the index
// catalog types built in Step 13.

pub use crate::storage::btree::{BTree, BtreeKey, RecordId};
pub use crate::storage::index_catalog::{EntityKind, IndexDef};
