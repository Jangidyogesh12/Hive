// Index store — currently re-exports the durable B+ tree implementation built
// in Step 12.  Step 13 will add label/property index types on top of this tree.

pub use crate::storage::btree::{BTree, BtreeKey, RecordId};
