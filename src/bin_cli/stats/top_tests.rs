use super::top::{
    AGGREGATE_ONLY_METRICS, append_cycle_units, collect_all_units, coverage_pct_map,
    decorate_file_units_with_coverage, extractor_for,
};
use super::top_roots::{runtime_coverage_root, stats_runtime_py_jobs};
use std::collections::{BTreeMap, HashMap};
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
fn stats_runtime_py_jobs_serializes_nested_llvm_cov_stats() {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap();
    let old = std::env::var_os("CARGO_LLVM_COV");
    unsafe { std::env::set_var("CARGO_LLVM_COV", "1") };

    assert_eq!(stats_runtime_py_jobs(), Some(1));

    match old {
        Some(value) => unsafe { std::env::set_var("CARGO_LLVM_COV", value) },
        None => unsafe { std::env::remove_var("CARGO_LLVM_COV") },
    }
}

#[test]
fn stats_runtime_py_jobs_uses_default_parallelism_outside_nested_coverage() {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap();
    let old_cov = std::env::var_os("CARGO_LLVM_COV");
    let old_target = std::env::var_os("CARGO_LLVM_COV_TARGET_DIR");
    unsafe { std::env::remove_var("CARGO_LLVM_COV") };
    unsafe { std::env::remove_var("CARGO_LLVM_COV_TARGET_DIR") };

    assert_eq!(stats_runtime_py_jobs(), None);

    match old_cov {
        Some(value) => unsafe { std::env::set_var("CARGO_LLVM_COV", value) },
        None => unsafe { std::env::remove_var("CARGO_LLVM_COV") },
    }
    match old_target {
        Some(value) => unsafe { std::env::set_var("CARGO_LLVM_COV_TARGET_DIR", value) },
        None => unsafe { std::env::remove_var("CARGO_LLVM_COV_TARGET_DIR") },
    }
}

#[test]
fn runtime_coverage_root_uses_analyzed_temp_project() {
    let tmp = tempfile::TempDir::new().unwrap();
    let source = tmp.path().join("pkg").join("module.py");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "def f():\n    return 1\n").unwrap();

    let input_paths = vec![tmp.path().to_string_lossy().to_string()];
    let root = runtime_coverage_root(&input_paths, &[source], &[]);

    assert_eq!(root, tmp.path());
}

#[test]
fn runtime_coverage_root_climbs_to_project_marker() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("pyproject.toml"),
        "[project]\nname = \"fixture\"\n",
    )
    .unwrap();
    let package = tmp.path().join("src").join("pkg");
    std::fs::create_dir_all(&package).unwrap();
    let source = package.join("module.py");
    std::fs::write(&source, "def f():\n    return 1\n").unwrap();

    let root = runtime_coverage_root(&[], &[source], &[]);

    assert_eq!(root, tmp.path());
}

#[test]
fn runtime_coverage_root_uses_common_parent_for_multiple_inputs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let left = tmp.path().join("pkg").join("left.py");
    let right = tmp.path().join("tests").join("test_left.py");
    std::fs::create_dir_all(left.parent().unwrap()).unwrap();
    std::fs::create_dir_all(right.parent().unwrap()).unwrap();
    std::fs::write(&left, "def f():\n    return 1\n").unwrap();
    std::fs::write(&right, "def test_f():\n    assert True\n").unwrap();
    let input_paths = vec![
        left.to_string_lossy().to_string(),
        right.to_string_lossy().to_string(),
    ];

    let root = runtime_coverage_root(&input_paths, &[left, right], &[]);

    assert_eq!(root, tmp.path());
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

fn file_stem_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

fn is_stats_check_shared_metric(metric_id: &str) -> bool {
    ![
        "cycle_size",
        "inv_test_coverage",
        "duplication",
        "orphan_module",
        "test_coverage",
        "fan_in",
        "fan_out",
        "dependency_depth",
        "positional_args",
    ]
    .contains(&metric_id)
}

type MetricKey = (String, String, String);

fn stats_unit_map(units: &[kiss::UnitMetrics]) -> BTreeMap<MetricKey, usize> {
    let mut out = BTreeMap::new();
    for unit in units {
        for def in kiss::METRICS {
            let Some(extractor) = extractor_for(def.metric_id) else {
                continue;
            };
            if !is_stats_check_shared_metric(def.metric_id) {
                continue;
            }
            let Some(value) = extractor(unit) else {
                continue;
            };
            if value > 0 {
                out.insert(
                    (
                        def.metric_id.to_string(),
                        file_stem_of(&unit.file),
                        unit.name.clone(),
                    ),
                    value,
                );
            }
        }
    }
    out
}

fn check_violation_map(violations: &[kiss::Violation]) -> BTreeMap<MetricKey, usize> {
    violations
        .iter()
        .filter(|v| is_stats_check_shared_metric(&v.metric))
        .map(|v| {
            (
                (
                    v.metric.clone(),
                    file_stem_of(&v.file.to_string_lossy()),
                    v.unit_name.clone(),
                ),
                v.value,
            )
        })
        .collect()
}

fn write_sync_corpus(root: &std::path::Path) {
    std::fs::write(
        root.join("module.py"),
        "import os\n\
         import json\n\
         \n\
         class DataProcessor:\n\
             def add(self, item):\n\
                 return item\n\
         \n\
         def complex_function(a, b, c, *, key=None, verbose: bool = False):\n\
             x = 1\n\
             y = 2\n\
             if a > b:\n\
                 return x\n\
             return y + c\n",
    )
    .unwrap();
}

#[test]
fn detailed_stats_and_check_share_metric_values() {
    let tmp = tempfile::TempDir::new().unwrap();
    write_sync_corpus(tmp.path());
    let paths = vec![tmp.path().to_string_lossy().to_string()];
    let (py_files, rs_files) =
        kiss::discovery::gather_files_by_lang(&paths, Some(kiss::Language::Python), &[]);

    let cached_coverage: HashMap<String, usize> = py_files
        .iter()
        .map(|path| (path.display().to_string(), 100))
        .collect();
    let stats_map = stats_unit_map(&collect_all_units(
        &py_files,
        &rs_files,
        Some(&cached_coverage),
    ));

    let py_cfg = kiss::Config {
        statements_per_function: 0,
        methods_per_class: 0,
        statements_per_file: 0,
        lines_per_file: 0,
        functions_per_file: 0,
        arguments_positional: 0,
        arguments_keyword_only: 0,
        max_indentation_depth: 0,
        interface_types_per_file: 0,
        concrete_types_per_file: 0,
        nested_function_depth: 0,
        returns_per_function: 0,
        return_values_per_function: 0,
        branches_per_function: 0,
        local_variables_per_function: 0,
        imported_names_per_file: 0,
        statements_per_try_block: 0,
        boolean_parameters: 0,
        annotations_per_function: 0,
        calls_per_function: 0,
        cycle_size: 0,
        indirect_dependencies: 0,
        dependency_depth: 0,
    };
    let rs_cfg = kiss::Config::rust_defaults();
    let gate = kiss::GateConfig {
        test_coverage_threshold: 0,
        min_similarity: 1.0,
        duplication_enabled: false,
        orphan_module_enabled: false,
    };
    let focus = crate::analyze::FocusFilter::unrestricted();
    let opts = crate::analyze::AnalyzeOptions {
        universe: &paths[0],
        focus_paths: &paths,
        py_config: &py_cfg,
        rs_config: &rs_cfg,
        lang_filter: Some(kiss::Language::Python),
        bypass_gate: true,
        gate_config: &gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: true,
        jobs: None,
    };
    let now = std::time::Instant::now();
    let pipeline = crate::analyze::run_full_pipeline(crate::analyze::FullPipelineInput {
        opts: &opts,
        py_files: &py_files,
        rs_files: &rs_files,
        focus: &focus,
        t0: now,
        t1: now,
        t2: now,
    });
    let mut check_violations = pipeline.result.violations;
    check_violations.extend(pipeline.graph_viols_all);

    assert_eq!(stats_map, check_violation_map(&check_violations));
}
