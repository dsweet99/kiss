use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(not(test))]
pub(crate) use super::rust_llvm_cov::run_rust_llvm_cov_selectors;
#[cfg(test)]
pub(crate) use super::rust_llvm_cov::run_rust_llvm_cov_selectors;
use kiss::test_refs::{is_in_test_directory, is_test_file};
use kiss::{parse_rust_files, rust_test_functions_in};
use rust_llvm_cov_runner::{
    CoverageOutputMode, RustCoverageBatchCounters, RustCoverageBatchRequest,
    build_rust_coverage_batch_plan,
};

#[path = "runners/decision.rs"]
mod decision;
pub(crate) use decision::combined_selectors;

#[path = "runners/python_backer.rs"]
pub(crate) mod python_backer;
#[path = "runners/python_collect.rs"]
mod python_collect;
#[cfg(test)]
#[path = "runners/python_collect_acceptance_test.rs"]
mod python_collect_acceptance_test;
#[cfg(test)]
#[path = "runners/python_collect_error_test.rs"]
mod python_collect_error_test;
use python_collect::collect_python_nodeids;
#[cfg(test)]
#[path = "runners/python_collect_test.rs"]
mod python_collect_test;
#[path = "runners/rust_backer.rs"]
pub(crate) mod rust_backer;

#[path = "runners/rslip.rs"]
mod rslip;
pub(crate) use rslip::run_rslip_selectors;
pub(crate) use rslip::{detect_rslip_versions, rslip_request_from_parts};

pub const NO_COVERING_TESTS_MSG: &str = "NO COVERING TESTS";

#[cfg(test)]
pub(crate) fn py_selector(test_path: &Path, test_id: &str) -> String {
    format!("{}::{}", test_path.display(), test_id)
}

pub fn merge_exit_codes(a: i32, b: i32) -> i32 {
    a.max(b)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SelectorExecutionSummary {
    pub(crate) exit_code: i32,
    pub(crate) total: usize,
    pub(crate) cache_hits: usize,
    pub(crate) cache_misses: usize,
    pub(crate) cache_unstored: usize,
    pub(crate) failed: usize,
    pub(crate) rust_build_invocations: usize,
    pub(crate) rust_test_instances: usize,
    pub(crate) rust_export_jobs: usize,
    pub(crate) rust_aggregate_binaries: usize,
    pub(crate) rust_aggregate_exports: usize,
    pub(crate) rust_batch_cache_hits: usize,
    pub(crate) rust_max_active_test_instances: usize,
    pub(crate) rust_max_active_exports: usize,
    pub(crate) rust_unmatched_selectors: usize,
    pub(crate) rust_max_objects_per_export: usize,
    pub(crate) rust_build_target_baseline_bytes: u64,
    pub(crate) phase_rust_export_ms: u128,
    pub(crate) rust_derived_state_published: bool,
    pub(crate) rust_derived_repair: bool,
    pub(crate) rust_entry_generation_count: usize,
    pub(crate) rust_current_index_generation: String,
    pub(crate) rust_cache_pruned_entries: usize,
    pub(crate) rust_process_residual_count: usize,
    pub(crate) rust_legacy_cleanup_deferred: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectorCacheRecord {
    Hit,
    MissStored,
    MissUnstored,
}

impl SelectorExecutionSummary {
    pub(crate) fn record(
        &mut self,
        status: rpytest_runner::TestStatus,
        cache_record: SelectorCacheRecord,
        exit_code: Option<i32>,
    ) {
        self.total += 1;
        match cache_record {
            SelectorCacheRecord::Hit => self.cache_hits += 1,
            SelectorCacheRecord::MissStored => self.cache_misses += 1,
            SelectorCacheRecord::MissUnstored => {
                self.cache_misses += 1;
                self.cache_unstored += 1;
            }
        }
        if status == rpytest_runner::TestStatus::Failed {
            self.failed += 1;
            self.exit_code = merge_exit_codes(self.exit_code, exit_code.unwrap_or(1));
        }
    }

    pub(crate) fn record_rust_batch_counters(&mut self, counters: &RustCoverageBatchCounters) {
        self.rust_build_invocations += counters.build_invocations;
        self.rust_test_instances += counters.test_instances;
        self.rust_export_jobs += counters.export_jobs;
        self.rust_aggregate_binaries += counters.aggregate_binaries;
        self.rust_aggregate_exports += counters.aggregate_exports;
        self.rust_batch_cache_hits += counters.cache_hits;
        self.rust_max_active_test_instances = self
            .rust_max_active_test_instances
            .max(counters.max_active_test_instances);
        self.rust_max_active_exports = self
            .rust_max_active_exports
            .max(counters.max_active_exports);
        self.rust_unmatched_selectors += counters.unmatched_selectors;
        self.rust_max_objects_per_export = self
            .rust_max_objects_per_export
            .max(counters.max_objects_per_export);
        self.rust_build_target_baseline_bytes = self
            .rust_build_target_baseline_bytes
            .max(counters.build_target_baseline_bytes);
        self.phase_rust_export_ms += counters.export_phase_ms;
        if counters.derived_state_published {
            self.rust_derived_state_published = true;
        }
        if counters.derived_repair {
            self.rust_derived_repair = true;
        }
        self.rust_entry_generation_count = self
            .rust_entry_generation_count
            .max(counters.entry_generation_count);
        if !counters.current_index_generation.is_empty() {
            self.rust_current_index_generation = counters.current_index_generation.clone();
        }
        self.rust_cache_pruned_entries += counters.cache_pruned_entries;
        self.rust_process_residual_count += counters.process_residual_count;
        if counters.legacy_cleanup_deferred {
            self.rust_legacy_cleanup_deferred = true;
        }
    }
}

pub fn partition_changed_paths(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut source = Vec::new();
    let mut test = Vec::new();
    for p in paths {
        let is_py = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("py"));
        let is_rs = is_rust_planning_source_path(p);
        if is_py {
            if is_test_file(p) || is_in_test_directory(p) {
                test.push(p.clone());
            } else {
                source.push(p.clone());
            }
        } else if is_rs {
            if kiss::is_rust_test_file(p) {
                test.push(p.clone());
            } else {
                source.push(p.clone());
            }
        }
    }
    (source, test)
}

pub(crate) fn is_rust_planning_source_path(path: &Path) -> bool {
    kiss::Language::is_rust_path(path) || rust_llvm_cov_runner::is_rust_cov_cache_input(path)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangedFileTests {
    pub python_nodeids: BTreeSet<String>,
    pub rust_tests: BTreeSet<(PathBuf, String)>,
}

pub fn enumerate_tests_in_changed_files(
    repo_root: &Path,
    test_paths: &[PathBuf],
) -> Result<ChangedFileTests, String> {
    let mut out = ChangedFileTests::default();
    let py: Vec<_> = test_paths
        .iter()
        .filter(|p| p.is_file())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("py")))
        .cloned()
        .collect();
    let rs: Vec<_> = test_paths
        .iter()
        .filter(|p| p.is_file())
        .filter(|p| kiss::Language::is_rust_path(p))
        .cloned()
        .collect();
    if !py.is_empty() {
        for nodeid in collect_python_nodeids(repo_root, Some(&py), &[])? {
            out.python_nodeids.insert(nodeid);
        }
    }
    if !rs.is_empty() {
        let parsed = parse_rust_files(&rs);
        for (path, r) in rs.iter().zip(parsed) {
            let pf = r.map_err(|e| {
                format!("error: kiss test: failed to parse {}: {e}", path.display())
            })?;
            let ids = rust_test_functions_in(&pf);
            for id in ids {
                out.rust_tests.insert((pf.path.clone(), id));
            }
        }
    }
    Ok(out)
}

pub fn enumerate_workspace_rust_selectors(
    repo_root: &Path,
    ignore: &[String],
) -> Result<Vec<String>, String> {
    let root = repo_root.to_string_lossy().to_string();
    let (_py_files, rs_files) =
        kiss::gather_files_by_lang(&[root], Some(kiss::Language::Rust), ignore);
    let parsed = parse_rust_files(&rs_files);
    let mut selectors = BTreeSet::new();
    for (path, result) in rs_files.iter().zip(parsed) {
        let pf = result.map_err(|e| {
            format!(
                "error: kiss test: failed to parse Rust workspace file {}: {e}",
                path.display()
            )
        })?;
        for selector in rust_test_functions_in(&pf) {
            selectors.insert(selector);
        }
    }
    Ok(selectors.into_iter().collect())
}

pub fn enumerate_workspace_python_selectors(
    repo_root: &Path,
    ignore: &[String],
) -> Result<Vec<String>, String> {
    if !ignore.is_empty() {
        let root = repo_root.to_string_lossy().to_string();
        let (py_files, _rs_files) =
            kiss::gather_files_by_lang(&[root], Some(kiss::Language::Python), ignore);
        let test_paths = py_files
            .into_iter()
            .filter(|path| is_test_file(path) || is_in_test_directory(path))
            .collect::<Vec<_>>();
        return collect_python_nodeids(repo_root, Some(&test_paths), &[]);
    }
    collect_python_nodeids(repo_root, None, &[])
}

pub fn shell_quote_line(argv: &[String]) -> String {
    argv.iter()
        .map(|a| shlex_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn shlex_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '=' | ','))
        && !s.starts_with('-')
    {
        return s.to_string();
    }
    let mut out = String::from('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\"'\"'");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

pub fn build_pytest_argv(selectors: &[String], extra: &[String]) -> Vec<String> {
    let mut v = vec!["python".into(), "-m".into(), "pytest".into()];
    v.extend(selectors.iter().cloned());
    v.extend(extra.iter().cloned());
    v
}

pub(crate) fn build_rust_coverage_batch_dry_run_lines(
    selectors: &[String],
    extra: &[String],
    jobs: usize,
) -> Result<Vec<String>, String> {
    if selectors.is_empty() {
        return Ok(Vec::new());
    }
    let (delegated_runners, runner_map_fingerprint, host_platform) =
        rust_llvm_cov_runner::placeholder_delegated_runner_fields();
    let req = RustCoverageBatchRequest {
        cwd: PathBuf::from("."),
        source_root: PathBuf::from("."),
        cargo: PathBuf::from("cargo"),
        cache_root: PathBuf::from("<cache>/rust_llvm_cov_cache"),
        logical_selectors: selectors.to_vec(),
        cargo_args: Vec::new(),
        test_args: extra.to_vec(),
        env: BTreeMap::new(),
        force_rerun: false,
        jobs,
        generated_config: PathBuf::from("<generated-filter>"),
        population_publication_selectors: None,
        delegated_runners,
        runner_map_fingerprint,
        host_platform,
        coverage_output_mode: CoverageOutputMode::SelectorEntries,
    };
    let plan = build_rust_coverage_batch_plan(&req)?;
    let mut lines = vec![
        format!("RUST BATCH selectors={} jobs={jobs}", selectors.len()),
        shell_quote_line(&plan.argv),
    ];
    lines.extend(
        selectors
            .iter()
            .map(|selector| format!("RUST SELECTOR {selector}")),
    );
    Ok(lines)
}

pub(crate) fn command_stdout(program: &Path, args: &[&str], cwd: &Path) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("failed to spawn {}: {e}", program.display()))?;
    if !output.status.success() {
        return Err(format!(
            "error: kiss test: {} failed: {}",
            program.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
