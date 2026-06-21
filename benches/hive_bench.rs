use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use hive::db::hive_db::{HiveDb, Property};
use hive::types::NodeId;
use hive::value::{self, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDb {
    path: PathBuf,
}

impl TempDb {
    fn new(name: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("hive_bench_{}_{}", name, stamp));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn integer_property(key: &str, value_int: i64) -> Property {
    let key_hash = value::hash_key(key);
    let (value_type, value_inline) = Value::Integer(value_int).to_inline_bytes();
    Property {
        key_value: key.to_string(),
        key_hash,
        value_type,
        value_inline,
    }
}

fn benchmark_node_insert(c: &mut Criterion) {
    c.bench_function("node_insert", |b| {
        b.iter_batched(
            || {
                let temp = TempDb::new("node_insert");
                let db = HiveDb::open(temp.path()).unwrap();
                (temp, db)
            },
            |(_temp, mut db)| {
                black_box(db.create_node("Person", vec![]).unwrap());
            },
            BatchSize::SmallInput,
        )
    });
}

fn benchmark_edge_insert(c: &mut Criterion) {
    c.bench_function("edge_insert", |b| {
        b.iter_batched(
            || {
                let temp = TempDb::new("edge_insert");
                let mut db = HiveDb::open(temp.path()).unwrap();
                let src = db.create_node("Person", vec![]).unwrap();
                let dst = db.create_node("Person", vec![]).unwrap();
                (temp, db, src, dst)
            },
            |(_temp, mut db, src, dst)| {
                black_box(db.create_edge(src, dst, "KNOWS", vec![]).unwrap());
            },
            BatchSize::SmallInput,
        )
    });
}

fn build_star_graph(size: usize) -> (TempDb, HiveDb, NodeId) {
    let temp = TempDb::new("one_hop");
    let mut db = HiveDb::open(temp.path()).unwrap();
    let center = db.create_node("Person", vec![]).unwrap();

    for _ in 0..size {
        let dst = db.create_node("Person", vec![]).unwrap();
        db.create_edge(center, dst, "KNOWS", vec![]).unwrap();
    }

    (temp, db, center)
}

fn benchmark_one_hop_traversal(c: &mut Criterion) {
    let (_temp, mut db, center) = build_star_graph(256);

    c.bench_function("one_hop_traversal", |b| {
        b.iter(|| black_box(db.get_out_neighbors(center).unwrap()))
    });
}

fn build_layered_graph(width: usize) -> (TempDb, HiveDb, NodeId) {
    let temp = TempDb::new("three_hop");
    let mut db = HiveDb::open(temp.path()).unwrap();
    let root = db.create_node("Person", vec![]).unwrap();
    let mut current = vec![root];

    for _ in 0..3 {
        let mut next = Vec::new();
        for &src in &current {
            for _ in 0..width {
                let dst = db.create_node("Person", vec![]).unwrap();
                db.create_edge(src, dst, "KNOWS", vec![]).unwrap();
                next.push(dst);
            }
        }
        current = next;
    }

    (temp, db, root)
}

fn traverse_three_hops(db: &mut HiveDb, root: NodeId) -> Vec<NodeId> {
    let mut frontier = vec![root];
    for _ in 0..3 {
        let mut next = Vec::new();
        for node_id in frontier {
            next.extend(db.get_out_neighbors(node_id).unwrap());
        }
        frontier = next;
    }
    frontier
}

fn benchmark_three_hop_traversal(c: &mut Criterion) {
    let (_temp, mut db, root) = build_layered_graph(8);

    c.bench_function("three_hop_traversal", |b| {
        b.iter(|| black_box(traverse_three_hops(&mut db, root)))
    });
}

fn build_lookup_fixture(size: usize) -> (TempDb, HiveDb, i64) {
    let temp = TempDb::new("lookup_fixture");
    let mut db = HiveDb::open(temp.path()).unwrap();
    let target = (size / 2) as i64;

    for i in 0..size {
        let props = vec![integer_property("user_id", i as i64)];
        db.create_node("Person", props).unwrap();
    }

    (temp, db, target)
}

fn full_scan_lookup(db: &mut HiveDb, target: i64) -> Vec<NodeId> {
    let mut matches = Vec::new();
    let expected = Value::Integer(target);
    let count = db.node_count().unwrap();

    for node_id in 0..count {
        if db
            .get_node_property(node_id, "user_id")
            .unwrap()
            .as_ref()
            == Some(&expected)
        {
            matches.push(node_id);
        }
    }

    matches
}

fn benchmark_indexed_lookup_vs_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("lookup_vs_scan");
    let (_indexed_temp, indexed_db, indexed_target) = build_lookup_fixture(1_000);
    let (_scan_temp, mut scan_db, scan_target) = build_lookup_fixture(1_000);

    group.bench_function("indexed_lookup", |b| {
        b.iter(|| {
            black_box(
                indexed_db
                    .lookup_node_ids_by_property(
                        "user_id",
                        &Value::Integer(indexed_target),
                    )
                    .unwrap(),
            );
        })
    });

    group.bench_function("full_scan_lookup", |b| {
        b.iter(|| black_box(full_scan_lookup(&mut scan_db, scan_target)))
    });

    group.finish();
}

fn benches(c: &mut Criterion) {
    benchmark_node_insert(c);
    benchmark_edge_insert(c);
    benchmark_one_hop_traversal(c);
    benchmark_three_hop_traversal(c);
    benchmark_indexed_lookup_vs_scan(c);
}

criterion_group!(hive_benches, benches);
criterion_main!(hive_benches);
