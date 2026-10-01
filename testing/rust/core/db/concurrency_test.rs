use crate::db::hive_db::HiveDb;
use crate::db::shared::SharedDb;
use crate::value::Value;
use std::sync::Barrier;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hive_conc_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn concurrent_readers_are_safe() {
    let dir = temp_dir("readers");
    let db = SharedDb::open(&dir).unwrap();
    db.execute(r#"CREATE (n:Person {name: "A"})"#).unwrap();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let db = db.clone();
            std::thread::spawn(move || {
                for _ in 0..25 {
                    let r = db
                        .execute_read(r#"MATCH (n:Person) RETURN n.name"#)
                        .unwrap();
                    assert_eq!(r.rows.len(), 1);
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_writers_are_exclusive_and_durable() {
    let dir = temp_dir("writers");
    let db = SharedDb::open(&dir).unwrap();

    let handles: Vec<_> = (0..4)
        .map(|worker| {
            let db = db.clone();
            std::thread::spawn(move || {
                for i in 0..10 {
                    let query = format!(r#"CREATE (n:Person {{name: "w{worker}_{i}"}})"#);
                    db.execute(&query).unwrap();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }

    let r = db.execute(r#"MATCH (n:Person) RETURN COUNT(*)"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(40)]]);
    assert!(db.check_integrity().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn execute_read_rejects_writes() {
    let dir = temp_dir("read_reject");
    let db = SharedDb::open(&dir).unwrap();
    let err = db.execute_read(r#"CREATE (n:Person)"#).unwrap_err();
    assert!(err.to_string().contains("read-only"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_sees_committed_data() {
    let dir = temp_dir("snap_committed");
    let mut db = HiveDb::open(&dir).unwrap();
    db.execute(r#"CREATE (:Person {age: 30})"#).unwrap();
    db.execute(r#"CREATE (:Person {age: 20})"#).unwrap();

    let mut snap = HiveDb::open_snapshot(&dir).unwrap();
    assert!(snap.is_snapshot());
    let r = snap
        .execute(r#"MATCH (n:Person) RETURN SUM(n.age)"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(50)]]);
    snap.close();
    db.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_is_isolated_from_later_writes() {
    let dir = temp_dir("snap_isolation");
    let mut db = HiveDb::open(&dir).unwrap();
    db.execute(r#"CREATE (:Person)"#).unwrap();
    db.execute(r#"CREATE (:Person)"#).unwrap();

    let mut stale = HiveDb::open_snapshot(&dir).unwrap();
    db.execute(r#"CREATE (:Person)"#).unwrap();

    // The earlier snapshot is frozen; a fresh one sees the new write.
    let r = stale
        .execute(r#"MATCH (n:Person) RETURN COUNT(*)"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(2)]]);
    stale.close();
    let mut fresh = HiveDb::open_snapshot(&dir).unwrap();
    let r = fresh
        .execute(r#"MATCH (n:Person) RETURN COUNT(*)"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(3)]]);
    fresh.close();
    db.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_unknown_names_match_nothing() {
    let dir = temp_dir("snap_unknown");
    let mut db = HiveDb::open(&dir).unwrap();
    db.execute(r#"CREATE (:Person {name: "A"})"#).unwrap();

    let mut snap = HiveDb::open_snapshot(&dir).unwrap();
    // Unknown labels/keys resolve without registering: empty, not an error.
    let r = snap.execute(r#"MATCH (n:Nope) RETURN n"#).unwrap();
    assert_eq!(r.rows.len(), 0);
    let r = snap
        .execute(r#"MATCH (n:Person) RETURN n.nonexistent"#)
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Null]]);
    snap.close();
    // And the main handle never learned those names.
    assert!(db.find_label("Nope").unwrap().is_none());
    db.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_rejects_writes_loudly() {
    let dir = temp_dir("snap_reject");
    let mut db = HiveDb::open(&dir).unwrap();
    db.execute(r#"CREATE (:Person)"#).unwrap();

    let mut snap = HiveDb::open_snapshot(&dir).unwrap();
    let err = snap.execute(r#"CREATE (:Person)"#).unwrap_err();
    assert!(err.to_string().contains("read-only snapshot"), "{err}");
    let err = snap.register_label("Anything").unwrap_err();
    assert!(err.to_string().contains("read-only snapshot"), "{err}");
    let err = snap.checkpoint().unwrap_err();
    assert!(err.to_string().contains("read-only snapshot"), "{err}");
    snap.close();
    // The failed writes left no trace.
    let r = db.execute(r#"MATCH (n:Person) RETURN COUNT(*)"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(1)]]);
    db.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn open_snapshot_missing_dir_errors() {
    let dir = std::env::temp_dir().join(format!("hive_conc_snap_missing_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let err = HiveDb::open_snapshot(&dir)
        .err()
        .expect("expected open error");
    assert!(err.to_string().contains("Failed to open"), "{err}");
}

#[test]
fn from_db_handles_have_no_snapshot_reads() {
    let dir = temp_dir("snap_fromdb");
    let mut db = HiveDb::open(&dir).unwrap();
    db.execute(r#"CREATE (:Person)"#).unwrap();
    let shared = SharedDb::from_db(db);
    let err = shared
        .execute_read(r#"MATCH (n:Person) RETURN n"#)
        .unwrap_err();
    assert!(
        err.to_string().contains("unknown database directory"),
        "{err}"
    );
    // The write path still works on such handles.
    shared.execute(r#"CREATE (:Person)"#).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_snapshot_reads_during_writes() {
    let dir = temp_dir("snap_hammer");
    let db = SharedDb::open(&dir).unwrap();
    for _ in 0..5 {
        db.execute(r#"CREATE (:Person)"#).unwrap();
    }

    let barrier = Barrier::new(7);
    std::thread::scope(|scope| {
        for _ in 0..6 {
            let db = db.clone();
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                let mut last = 5;
                for _ in 0..40 {
                    let r = db
                        .execute_read(r#"MATCH (n:Person) RETURN COUNT(*)"#)
                        .unwrap();
                    let count = match &r.rows[0][0] {
                        Value::Integer(n) => *n,
                        other => panic!("expected integer count, got {other:?}"),
                    };
                    // Snapshots only move forward: every read observes a
                    // committed prefix of the writer's work, never torn state.
                    assert!((5..=35).contains(&count), "{count}");
                    assert!(count >= last, "{count} < {last}");
                    last = count;
                }
            });
        }
        {
            let db = db.clone();
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                for _ in 0..30 {
                    db.execute(r#"CREATE (:Person)"#).unwrap();
                }
            });
        }
    });

    let r = db.execute(r#"MATCH (n:Person) RETURN COUNT(*)"#).unwrap();
    assert_eq!(r.rows, vec![vec![Value::Integer(35)]]);
    assert!(db.check_integrity().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
