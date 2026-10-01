use crate::db::hive_db::HiveDb;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hive_inspect_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn seeded_db(name: &str) -> (HiveDb, std::path::PathBuf) {
    let dir = temp_dir(name);
    let mut db = HiveDb::open(&dir).unwrap();
    db.execute(
        r#"CREATE (a:Person:Employee {name: "Alice", age: 30})-[:KNOWS {since: 2020}]->(b:Person {name: "Bob"})"#,
    )
    .unwrap();
    db.create_node_label_index("Person").unwrap();
    (db, dir)
}

#[test]
fn inspect_meta_page_reports_header() {
    let (mut db, dir) = seeded_db("meta");
    let report = db.inspect_page(0).unwrap();
    assert_eq!(report.page_type, "Meta");
    assert!(
        report.detail.iter().any(|l| l == "magic valid: true"),
        "{report}"
    );
    assert!(
        report.detail.iter().any(|l| l.starts_with("version: ")),
        "{report}"
    );
    assert!(
        report.detail.iter().any(|l| l.contains("node_count: 2")),
        "{report}"
    );
    db.close();
    cleanup(&dir);
}

#[test]
fn inspect_data_pages_decode_records() {
    let (mut db, dir) = seeded_db("data");
    let stats = db.stats().unwrap();
    // Find a node page by scanning reports.
    let mut saw_node = false;
    let mut saw_edge = false;
    let mut saw_label_dict = false;
    for page_id in 1..stats.page_count {
        let report = db.inspect_page(page_id).unwrap();
        assert_eq!(report.checksum_valid, Some(true), "{report}");
        for line in &report.detail {
            if line.contains("node id=") && line.contains("labels=[Person,Employee]") {
                saw_node = true;
            }
            if line.contains("edge id=") && line.contains("type=KNOWS") {
                saw_edge = true;
            }
            if line.contains("name=\"Person\"") {
                saw_label_dict = true;
            }
        }
    }
    assert!(saw_node, "expected a decoded multi-label node record");
    assert!(saw_edge, "expected a decoded edge record");
    assert!(saw_label_dict, "expected decoded label dictionary entries");
    db.close();
    cleanup(&dir);
}

#[test]
fn inspect_btree_and_index_pages() {
    let (mut db, dir) = seeded_db("btree");
    let stats = db.stats().unwrap();
    let mut saw_btree = false;
    for page_id in 1..stats.page_count {
        let report = db.inspect_page(page_id).unwrap();
        if report.page_type == "IndexLeaf" || report.page_type == "IndexInterior" {
            saw_btree = true;
            assert!(
                report.detail.iter().any(|l| l.contains("cells=")),
                "{report}"
            );
        }
    }
    assert!(saw_btree, "expected at least one B-tree index page");
    db.close();
    cleanup(&dir);
}

#[test]
fn inspect_out_of_range_errors() {
    let (mut db, dir) = seeded_db("oob");
    let err = db.inspect_page(999_999).unwrap_err();
    assert!(err.to_string().contains("out of range"), "{err}");
    db.close();
    cleanup(&dir);
}

#[test]
fn inspect_wal_lists_entries_with_limit() {
    let (mut db, dir) = seeded_db("wal");
    let all = db.inspect_wal(None).unwrap();
    assert!(!all.is_empty(), "expected WAL entries after writes");
    assert!(all.iter().any(|e| e.kind == "Commit"), "{all:?}");
    assert!(
        all.iter()
            .any(|e| e.kind == "PageImage" && e.page_id.is_some()),
        "{all:?}"
    );
    // Indices ascend; limit takes the most recent entries.
    for w in all.windows(2) {
        assert!(w[0].index < w[1].index);
        assert!(w[0].lsn <= w[1].lsn);
    }
    let tail = db.inspect_wal(Some(2)).unwrap();
    assert_eq!(tail.len(), 2.min(all.len()));
    assert_eq!(tail.last().unwrap().index, all.last().unwrap().index);
    db.close();
    cleanup(&dir);
}

#[test]
fn timed_execute_reports_rows_and_stages() {
    let (mut db, dir) = seeded_db("timed");
    let (result, metrics) = db.execute_timed("MATCH (n:Person) RETURN n.name").unwrap();
    assert_eq!(metrics.rows, result.rows.len());
    assert_eq!(metrics.rows, 2);
    assert_eq!(
        format!("{metrics}"),
        format!(
            "2 rows in {:.3}ms (parse {:.3}ms, plan {:.3}ms, execute {:.3}ms)",
            metrics.total().as_secs_f64() * 1000.0,
            metrics.parse.as_secs_f64() * 1000.0,
            metrics.plan.as_secs_f64() * 1000.0,
            metrics.execute.as_secs_f64() * 1000.0,
        )
    );
    let shared = format!("{metrics}");
    assert!(shared.starts_with("2 rows in "), "{shared}");
    db.close();
    cleanup(&dir);
}

#[test]
fn snapshot_meta_magic_survives_commits() {
    // Regression test: LSN stamping once destroyed page-0 magic in cache
    // and on disk. Commits must leave the meta magic valid everywhere.
    let dir = temp_dir("magic");
    let mut db = HiveDb::open(&dir).unwrap();
    for i in 0..5 {
        db.execute(&format!("CREATE (:Person {{age: {i}}})"))
            .unwrap();
    }
    let report = db.inspect_page(0).unwrap();
    assert!(
        report.detail.iter().any(|l| l == "magic valid: true"),
        "{report}"
    );
    db.close();
    let raw = std::fs::read(dir.join("hive.db")).unwrap();
    assert_eq!(&raw[0..4], b"HIVE");
    let mut db = HiveDb::open(&dir).unwrap();
    let report = db.inspect_page(0).unwrap();
    assert!(
        report.detail.iter().any(|l| l == "magic valid: true"),
        "{report}"
    );
    db.close();
    cleanup(&dir);
}

fn cleanup(dir: &std::path::Path) {
    let _ = std::fs::remove_dir_all(dir);
}
