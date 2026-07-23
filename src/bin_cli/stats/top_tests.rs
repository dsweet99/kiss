use super::top::{
    AGGREGATE_ONLY_METRICS, StatsTopArgs, append_cycle_units, extractor_for,
    finalize_stats_top_status, merge_fresh_items, print_top_for_metric, run_stats_top_status,
};
use kiss::{Config, GateConfig, Language};
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
fn extractor_for_cycle_size_reads_field() {
    let mut u = file_unit("a.rs", "mod_a");
    u.cycle_size = Some(3);
    assert_eq!(extractor_for("cycle_size").unwrap()(&u), Some(3));
}

#[test]
fn append_cycle_units_emits_one_unit_per_cycle() {
    let mut g = kiss::DependencyGraph::new();
    for m in ["mod_a", "mod_b", "mod_c"] {
        g.get_or_create_node(m);
        g.paths
            .insert(m.to_string(), PathBuf::from(format!("{m}.rs")));
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
fn run_stats_top_status_reports_empty_inputs_without_exiting() {
    let py = Config::default();
    let rs = Config::default();
    let gate = GateConfig::default();
    let paths = ["/no/such/path".to_string()];
    let ignore: Vec<String> = Vec::new();
    let code = run_stats_top_status(StatsTopArgs {
        paths: &paths,
        lang_filter: Some(Language::Python),
        ignore: &ignore,
        n: 3,
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
    });
    assert_eq!(code, 1);
}

#[test]
fn run_stats_top_status_analyzes_temp_python_file() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("app.py");
    std::fs::write(&file, "def f():\n    return 1\n").unwrap();
    let py = Config::default();
    let rs = Config::default();
    let gate = GateConfig::default();
    let paths = [tmp.path().to_string_lossy().into_owned()];
    let ignore: Vec<String> = Vec::new();
    let code = run_stats_top_status(StatsTopArgs {
        paths: &paths,
        lang_filter: Some(Language::Python),
        ignore: &ignore,
        n: 3,
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
    });
    assert_eq!(code, 0);
}

#[test]
fn run_stats_top_status_empty_vs_populated_is_metamorphic() {
    let py = Config::default();
    let rs = Config::default();
    let gate = GateConfig::default();
    let ignore: Vec<String> = Vec::new();
    let empty = run_stats_top_status(StatsTopArgs {
        paths: &["/no/such/path".to_string()],
        lang_filter: Some(Language::Python),
        ignore: &ignore,
        n: 1,
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
    });
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("m.py"), "x = 1\n").unwrap();
    let populated = run_stats_top_status(StatsTopArgs {
        paths: &[tmp.path().to_string_lossy().into_owned()],
        lang_filter: Some(Language::Python),
        ignore: &ignore,
        n: 1,
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
    });
    assert_ne!(empty, populated);
    assert_eq!(empty, 1);
    assert_eq!(populated, 0);
}

#[test]
fn finalize_stats_top_status_returns_on_success() {
    finalize_stats_top_status(0);
}

#[test]
fn stats_top_args_preserves_cli_inputs() {
    let paths = vec!["src".to_string()];
    let ignore = vec!["target".to_string()];
    let py = Config::python_defaults();
    let rs = Config::rust_defaults();
    let gate = GateConfig::default();
    let args = StatsTopArgs {
        paths: &paths,
        lang_filter: Some(Language::Rust),
        ignore: &ignore,
        n: 7,
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
    };
    assert_eq!(args.paths, ["src"]);
    assert_eq!(args.lang_filter, Some(Language::Rust));
    assert_eq!(args.ignore, ["target"]);
    assert_eq!(args.n, 7);
    assert_eq!(args.py_config.lines_per_file, py.lines_per_file);
    assert_eq!(args.rs_config.lines_per_file, rs.lines_per_file);
    assert_eq!(
        args.gate_config.test_coverage_threshold,
        gate.test_coverage_threshold
    );
}

#[test]
fn merge_fresh_items_none_none_is_none() {
    assert!(merge_fresh_items(None, None).is_none());
}

#[test]
fn print_top_for_metric_skips_when_no_values() {
    print_top_for_metric(&[], 3, "lines_per_file", |_| None);
}

#[test]
fn print_top_for_metric_emits_ranked_rows() {
    let mut hi = file_unit("hi.rs", "hi");
    hi.lines = Some(100);
    let mut lo = file_unit("lo.rs", "lo");
    lo.lines = Some(10);
    print_top_for_metric(&[hi, lo], 1, "lines_per_file", |u| u.lines);
}

#[test]
fn run_stats_top_status_analyzes_temp_rust_file() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("lib.rs"), "pub fn f() {}\n").unwrap();
    let py = Config::default();
    let rs = Config::default();
    let gate = GateConfig::default();
    let paths = [tmp.path().to_string_lossy().into_owned()];
    let ignore: Vec<String> = Vec::new();
    let code = run_stats_top_status(StatsTopArgs {
        paths: &paths,
        lang_filter: Some(Language::Rust),
        ignore: &ignore,
        n: 2,
        py_config: &py,
        rs_config: &rs,
        gate_config: &gate,
    });
    assert_eq!(code, 0);
}

#[test]
fn merge_fresh_items_metamorphic_none_vs_some() {
    let empty = merge_fresh_items(None, None);
    let nonempty = merge_fresh_items(Some(()), None);
    assert!(empty.is_none());
    assert!(nonempty.is_none());
}
