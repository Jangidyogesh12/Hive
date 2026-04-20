//! Shared helpers for creating and cleaning temporary files in tests.
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn temp_file(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("hive_{}_{}.db", name, stamp));
    p
}

pub fn cleanup_file(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}
