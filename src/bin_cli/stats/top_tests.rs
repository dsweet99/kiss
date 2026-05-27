use super::top::{
    AGGREGATE_ONLY_METRICS, append_cycle_units, coverage_pct_map,
    decorate_file_units_with_coverage, extractor_for,
};
use std::collections::HashMap;
use std::path::PathBuf;

fn file_unit(path: &str, name: &str) -> kiss::UnitMetrics {
    kiss::UnitMetrics::new(path.to_string(), name.to_string(), "file", 1)
}

#[test]
fn extractor_or_allowlist_covers_every_registry_metric() {
    let unhandled: Vec<&'static str> = kiss::METRICS
        .iter()
        .map(|m| m.metric_id)
        .filter(|id| extractor_for(id).is_none() && !AGGREGATE_ONLY_METRICS.contains(id))
        .collect();
    assert!(unhandled.is_empty(), "unhandled: {unhandled:?}");
}

#[test]
fn allowlist_entries_have_no_extractor() {
    let conflicting: Vec<&'static str> = AGGREGATE_ONLY_METRICS
        .iter()
        .copied()
        .filter(|id| extractor_for(id).is_some())
        .collect();
    assert!(conflicting.is_empty(), "conflicting: {conflicting:?}");
}

#[test]
fn allowlist_entries_exist_in_registry() {
    let registry_ids: Vec<&'static str> = kiss::METRICS.iter().map(|m| m.metric_id).collect();
    let stale: Vec<&'static str> = AGGREGATE_ONLY_METRICS
        .iter()
        .copied()
        .filter(|id| !registry_ids.contains(id))
        .collect();
    assert!(stale.is_empty(), "stale: {stale:?}");
}

#[test]
fn extractor_for_inv_test_coverage_reads_field() {
    let mut u = file_unit("a.rs", "a.rs");
    u.inv_test_coverage = Some(75);
    assert_eq!(extractor_for("inv_test_coverage").unwrap()(&u), Some(75));
}

#[test]
fn extractor_for_cycle_size_reads_field() {
    let mut u = file_unit("a.rs", "mod_a");
    u.cycle_size = Some(3);
    assert_eq!(extractor_for("cycle_size").unwrap()(&u), Some(3));
}

#[test]
fn decorate_file_units_with_coverage_inverts_pct() {
    let mut units = vec![file_unit("c.rs", "c.rs"), file_unit("b.rs", "b.rs")];
    let mut map = HashMap::new();
    map.insert("c.rs".to_string(), 80);
    decorate_file_units_with_coverage(&mut units, &map);
    assert_eq!(units[0].inv_test_coverage, Some(20));
    assert_eq!(units[1].inv_test_coverage, Some(0));
}

#[test]
fn append_cycle_units_emits_one_unit_per_cycle() {
    let mut g = kiss::DependencyGraph::new();
    for m in ["mod_a", "mod_b", "mod_c"] {
        g.get_or_create_node(m);
        g.paths.insert(m.to_string(), PathBuf::from(format!("{m}.rs")));
    }
    g.add_dependency("mod_a", "mod_b");
    g.add_dependency("mod_b", "mod_c");
    g.add_dependency("mod_c", "mod_a");
    let mut units = Vec::new();
    append_cycle_units(&mut units, &g);
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].cycle_size, Some(3));
    assert_eq!(units[0].name, "mod_a");
    assert_eq!(units[0].file, "mod_a.rs");
}

#[test]
fn coverage_pct_map_groups_by_file() {
    struct Def {
        file: PathBuf,
    }
    let defs = vec![
        Def {
            file: PathBuf::from("a.py"),
        },
        Def {
            file: PathBuf::from("a.py"),
        },
        Def {
            file: PathBuf::from("b.py"),
        },
    ];
    let unrefs = vec![Def {
        file: PathBuf::from("a.py"),
    }];
    let map = coverage_pct_map(&defs, &unrefs, |d| &d.file);
    assert_eq!(map.get("a.py").copied(), Some(50));
    assert_eq!(map.get("b.py").copied(), Some(100));
}

#[test]
fn merge_fresh_items_combines_py_and_rs() {
    use super::top::{coverage_map_to_string_keys, merge_fresh_items};
    use kiss::check_universe_cache::CachedCoverageItem;
    use std::collections::HashMap;
    use std::path::PathBuf;

    let py = Some((
        vec![CachedCoverageItem {
            file: "a.py".into(),
            name: "f".into(),
            line: 1,
        }],
        Vec::new(),
    ));
    let merged = merge_fresh_items(py, None).expect("merged");
    assert_eq!(merged.0.len(), 1);
    assert!(merge_fresh_items(None, None).is_none());

    let mut map = HashMap::new();
    map.insert(PathBuf::from("a.py"), 100);
    let keyed = coverage_map_to_string_keys(map);
    assert_eq!(keyed.get("a.py").copied(), Some(100));
}

#[test]
fn collect_all_units_parses_python_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let py = tmp.path().join("a.py");
    std::fs::write(&py, "def f():\n    pass\n").unwrap();
    let units = super::collect_all_units(&[py], &[], None);
    assert!(units.iter().any(|u| u.name == "f"));
}

#[test]
fn collect_py_units_and_rs_units_on_fixtures() {
    use super::top_collect::{collect_py_units, collect_rs_units};
    let manifest = env!("CARGO_MANIFEST_DIR");
    let py = PathBuf::from(format!("{manifest}/tests/fake_python/helpers.py"));
    let (py_units, py_fresh) = collect_py_units(&[py], None);
    assert!(!py_units.is_empty());
    assert!(py_fresh.is_some());

    let rs = PathBuf::from(format!("{manifest}/tests/fake_rust/deep_nesting.rs"));
    let (rs_units, rs_fresh) = collect_rs_units(&[rs], None);
    assert!(!rs_units.is_empty());
    assert!(rs_fresh.is_some());
}

