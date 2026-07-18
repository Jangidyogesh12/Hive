use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut p = std::env::temp_dir();
    p.push(format!("hive_{}_{}", name, stamp));
    p
}

pub fn temp_dir(name: &str) -> PathBuf {
    let p = temp_path(name);
    let _ = std::fs::create_dir_all(&p);
    p
}

pub fn cleanup_dir(path: &std::path::Path) {
    let _ = std::fs::remove_dir_all(path);
}
