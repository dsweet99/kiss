
use super::*;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct TestFixture {
    py_cfg: kiss::Config,
    rs_cfg: kiss::Config,
    gate: GateConfig,
    focus: Vec<String>,
}

impl TestFixture {
    fn new() -> Self {
        Self {
            py_cfg: kiss::Config::python_defaults(),
            rs_cfg: kiss::Config::rust_defaults(),
            gate: GateConfig::default(),
            focus: vec![],
        }
    }

    fn make_opts(&self) -> crate::analyze::options::AnalyzeOptions<'_> {
        crate::analyze::options::AnalyzeOptions {
            universe: "/tmp",
            focus_paths: &self.focus,
            py_config: &self.py_cfg,
            rs_config: &self.rs_cfg,
            lang_filter: None,
            bypass_gate: false,
            gate_config: &self.gate,
            ignore_prefixes: &[],
            show_timing: false,
            suppress_final_status: false,
            jobs: None,
        }
    }

    fn with_input<R>(&self, f: impl FnOnce(&GatedPyParallelIn<'_>) -> R) -> R {
        let opts = self.make_opts();
        let input = GatedPyParallelIn {
            py_parsed: &[],
            opts: &opts,
            file_count: 0,
            gate: &self.gate,
        };
        f(&input)
    }
}

fn parsed_rs(path: PathBuf) -> ParsedRustFile {
    let source = "pub fn covered() -> usize { 1 }\n";
    let ast = syn::parse_file(source).unwrap();
    ParsedRustFile {
        path,
        source: source.to_string(),
        ast,
    }
}

fn parsed_py(path: PathBuf) -> ParsedFile {
    let mut parser = kiss::parsing::create_parser().unwrap();
    let source = "def covered():\n    return 1\n".to_string();
    let tree = parser.parse(&source, None).unwrap();
    ParsedFile { path, source, tree }
}

#[test]
fn repo_root_for_universe_canonicalizes_existing_roots() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("repo")).unwrap();
    let with_dot = tmp.path().join("repo").join(".");

    assert_eq!(
        repo_root_for_universe(with_dot.to_str().unwrap()),
        tmp.path().join("repo")
    );
}

#[test]
fn repo_root_for_universe_preserves_missing_roots() {
    let tmp = tempfile::TempDir::new().unwrap();
    let missing = tmp.path().join("missing-repo");

    assert_eq!(repo_root_for_universe(missing.to_str().unwrap()), missing);
}

#[test]
fn gated_py_parallel_input_preserves_gate_options() {
    let fix = TestFixture::new();
    fix.with_input(|input| {
        assert_eq!(input.file_count, 0);
        assert!(input.py_parsed.is_empty());
        assert!(std::ptr::eq(input.gate, input.opts.gate_config));
    });
}

#[test]
fn test_run_gated_analysis_empty_repo() {
    use crate::analyze::FocusFilter;
    use crate::analyze::params::GatedAnalysis;
    use crate::analyze_parse::ParseResult;
    use std::time::Instant;

    let fix = TestFixture::new();
    let opts = fix.make_opts();
    let focus = FocusFilter::unrestricted();
    let result = ParseResult {
        py_parsed: vec![],
        rs_parsed: vec![],
        violations: vec![],
        code_unit_count: 0,
        statement_count: 0,
    };
    let now = Instant::now();
    let analysis = GatedAnalysis {
        opts: &opts,
        py_files: &[],
        rs_files: &[],
        focus: &focus,
        parsed: (result, vec![], 0),
        timings: (now, now, now),
    };
    let outcome = run_gated_analysis(analysis);
    assert!(outcome.success);
}

#[test]
fn test_run_gated_analysis_fails_closed_when_py_runtime_refresh_fails() {
    use crate::analyze::FocusFilter;
    use crate::analyze::params::GatedAnalysis;
    use crate::analyze_parse::ParseResult;
    use std::time::Instant;

    let tmp = tempfile::TempDir::new().unwrap();
    let missing_repo = tmp.path().join("missing-repo");
    let fix = TestFixture::new();
    let opts = crate::analyze::options::AnalyzeOptions {
        universe: missing_repo.to_str().unwrap(),
        focus_paths: &fix.focus,
        py_config: &fix.py_cfg,
        rs_config: &fix.rs_cfg,
        lang_filter: None,
        bypass_gate: false,
        gate_config: &fix.gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: Some(1),
    };
    let py_file = parsed_py(missing_repo.join("pkg/a.py"));
    let focus = FocusFilter::unrestricted();
    let result = ParseResult {
        py_parsed: vec![py_file],
        rs_parsed: vec![],
        violations: vec![],
        code_unit_count: 1,
        statement_count: 2,
    };
    let now = Instant::now();
    let analysis = GatedAnalysis {
        opts: &opts,
        py_files: &[],
        rs_files: &[],
        focus: &focus,
        parsed: (result, vec![], 1),
        timings: (now, now, now),
    };

    let outcome = run_gated_analysis(analysis);

    assert!(!outcome.success);
    assert!(outcome.metrics.is_none());
}

#[test]
fn test_run_gated_analysis_finishes_rust_graph_when_runtime_cov_is_nested() {
    use crate::analyze::FocusFilter;
    use crate::analyze::params::GatedAnalysis;
    use crate::analyze_parse::ParseResult;
    use std::time::Instant;

    let _guard = ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("CARGO_LLVM_COV", "1") };
    let tmp = tempfile::TempDir::new().unwrap();
    let mut fix = TestFixture::new();
    fix.gate.test_coverage_threshold = 0;
    fix.gate.duplication_enabled = false;
    fix.gate.orphan_module_enabled = false;
    let opts = crate::analyze::options::AnalyzeOptions {
        universe: tmp.path().to_str().unwrap(),
        focus_paths: &fix.focus,
        py_config: &fix.py_cfg,
        rs_config: &fix.rs_cfg,
        lang_filter: None,
        bypass_gate: false,
        gate_config: &fix.gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
    };
    let rust_file = parsed_rs(tmp.path().join("src/lib.rs"));
    let focus = FocusFilter::unrestricted();
    let result = ParseResult {
        py_parsed: vec![],
        rs_parsed: vec![rust_file],
        violations: vec![],
        code_unit_count: 1,
        statement_count: 1,
    };
    let now = Instant::now();
    let analysis = GatedAnalysis {
        opts: &opts,
        py_files: &[],
        rs_files: &[],
        focus: &focus,
        parsed: (result, vec![], 1),
        timings: (now, now, now),
    };

    let outcome = run_gated_analysis(analysis);

    unsafe { std::env::remove_var("CARGO_LLVM_COV") };
    assert!(outcome.success);
    assert_eq!(outcome.metrics.unwrap().files, 1);
}

#[test]
fn test_gated_py_parallel_empty() {
    let fix = TestFixture::new();
    fix.with_input(|input| {
        let (py_cov, py_graph, graph_viols, py_dups) = gated_py_parallel(input);
        assert!(py_cov.definitions.is_empty());
        assert!(py_graph.is_none());
        assert!(graph_viols.is_empty());
        assert!(py_dups.is_empty());
    });
}

#[test]
fn test_gated_py_parallel_respects_disabled_duplication() {
    let mut fix = TestFixture::new();
    fix.gate.duplication_enabled = false;
    fix.with_input(|input| {
        let (_py_cov, _py_graph, _graph_viols, py_dups) = gated_py_parallel(input);
        assert!(py_dups.is_empty());
    });
}

#[test]
fn runtime_rust_coverage_returns_empty_during_nested_coverage_runs() {
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("CARGO_LLVM_COV", "1") };
    let fix = TestFixture::new();
    let missing_root = "/tmp/kiss-runtime-rust-coverage-nested-missing-root";
    let opts = crate::analyze::options::AnalyzeOptions {
        universe: missing_root,
        focus_paths: &fix.focus,
        py_config: &fix.py_cfg,
        rs_config: &fix.rs_cfg,
        lang_filter: None,
        bypass_gate: false,
        gate_config: &fix.gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
    };
    let parsed = vec![parsed_rs(PathBuf::from(missing_root).join("src/lib.rs"))];

    let analysis = runtime_rust_coverage_for_opts(&opts, &parsed);

    unsafe { std::env::remove_var("CARGO_LLVM_COV") };
    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn runtime_rust_coverage_allows_empty_input_without_cargo_probe() {
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe { std::env::remove_var("CARGO_LLVM_COV") };
    unsafe { std::env::remove_var("CARGO_LLVM_COV_TARGET_DIR") };
    let tmp = tempfile::TempDir::new().unwrap();
    let fix = TestFixture::new();
    let opts = crate::analyze::options::AnalyzeOptions {
        universe: tmp.path().to_str().unwrap(),
        focus_paths: &fix.focus,
        py_config: &fix.py_cfg,
        rs_config: &fix.rs_cfg,
        lang_filter: None,
        bypass_gate: false,
        gate_config: &fix.gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
    };

    let analysis = runtime_rust_coverage_for_opts(&opts, &[]);

    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
    assert!(analysis.coverage_map.is_empty());
}

#[test]
fn runtime_rust_coverage_canonicalizes_existing_universe() {
    let _guard = ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("CARGO_LLVM_COV", "1") };
    let tmp = tempfile::TempDir::new().unwrap();
    let repo_with_dot = tmp.path().join(".");
    let fix = TestFixture::new();
    let universe = repo_with_dot.to_str().unwrap();
    let opts = crate::analyze::options::AnalyzeOptions {
        universe,
        focus_paths: &fix.focus,
        py_config: &fix.py_cfg,
        rs_config: &fix.rs_cfg,
        lang_filter: None,
        bypass_gate: false,
        gate_config: &fix.gate,
        ignore_prefixes: &[],
        show_timing: false,
        suppress_final_status: false,
        jobs: None,
    };
    let parsed = vec![parsed_rs(tmp.path().join("src/lib.rs"))];

    let analysis = runtime_rust_coverage_for_opts(&opts, &parsed);

    unsafe { std::env::remove_var("CARGO_LLVM_COV") };
    assert!(analysis.definitions.is_empty());
    assert!(analysis.unreferenced.is_empty());
    assert!(analysis.coverage_map.is_empty());
    assert_eq!(repo_root_for_universe(universe), tmp.path());
}
