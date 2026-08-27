use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) use super::rust_llvm_cov::{
    cached_rust_check_aggregate_selectors, run_rust_llvm_cov_selectors,
};
use kiss::code_roles::{is_default_pytest_collect_candidate, is_test_only_file};
use kiss::rust_llvm_cov_runner::{
    CoverageOutputMode, RustCoverageBatchRequest, build_rust_coverage_batch_plan,
};

#[path = "runners/decision.rs"]
mod decision;
#[cfg(test)]
pub(crate) use decision::combined_selectors;
pub(crate) use decision::{
    CombinedSelectorInput, SelectorPlan, combined_selectors_with_direct,
    prior_failures_for_language,
};

#[path = "runners/rust_enumerate.rs"]
mod rust_enumerate;
pub use rust_enumerate::enumerate_workspace_rust_selectors;
pub(crate) use rust_enumerate::rust_logical_to_kiss_test_ids;

pub(crate) use crate::test_runner::lang_python::backer as python_backer;
pub(crate) use crate::test_runner::lang_python::collect;
use crate::test_runner::python_coverage_index::{
    PYTHON_COVERAGE_ENV_KEYS, repo_relative_path as python_repo_relative_path,
    stored_python_universe_selectors,
};
pub(crate) use collect::clear_python_collect_memo;
use collect::collect_python_nodeids;
pub(crate) fn collect_python_nodeids_for_targets(
    repo_root: &Path,
    paths: Option<&[PathBuf]>,
    pytest_args: &[String],
) -> Result<Vec<String>, String> {
    collect_python_nodeids(repo_root, paths, pytest_args)
}
pub(crate) use crate::test_runner::lang_rust::backer as rust_backer;

use crate::test_runner::lang_rust::workspace::{
    cargo_workspace_member_manifest_dirs, is_workspace_rust_selector_file,
};

pub(crate) use crate::test_runner::lang_python::rslip::run_rslip_selectors;
pub(crate) use crate::test_runner::lang_python::rslip::{
    detect_rslip_versions, rslip_request_from_parts,
};

#[path = "runners/execution_summary.rs"]
mod execution_summary;
#[path = "runners/rust_batch_counters.rs"]
mod rust_batch_counters;
pub(crate) use execution_summary::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary,
};

pub const NO_COVERING_TESTS_MSG: &str = "NO COVERING TESTS";

#[cfg(test)]
pub(crate) fn py_selector(test_path: &Path, test_id: &str) -> String {
    format!("{}::{}", test_path.display(), test_id)
}

pub fn merge_exit_codes(a: i32, b: i32) -> i32 {
    a.max(b)
}

#[cfg(test)]
pub fn partition_changed_paths(
    paths: &[PathBuf],
) -> Result<(Vec<PathBuf>, Vec<PathBuf>), kiss::code_roles::RoleBuildError> {
    let existing: Vec<PathBuf> = paths.iter().filter(|p| p.exists()).cloned().collect();
    let roles = roles_for_changed_paths(&existing)?;
    Ok(partition_changed_paths_with_roles(paths, &roles))
}

pub(crate) fn partition_changed_paths_with_roles(
    paths: &[PathBuf],
    roles: &kiss::code_roles::SourceRoleIndex,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut source = Vec::new();
    let mut test = Vec::new();
    for p in paths {
        let is_py = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("py"));
        let is_rs = is_rust_planning_source_path(p);
        if !is_py && !is_rs {
            continue;
        }
        if !p.exists() {
            source.push(p.clone());
            continue;
        }
        if is_test_only_file(roles, p) {
            test.push(p.clone());
        } else {
            source.push(p.clone());
        }
    }
    (source, test)
}

pub(crate) fn roles_for_universe(
    repo_root: &Path,
    ignore: &[String],
) -> Result<kiss::code_roles::SourceRoleIndex, kiss::code_roles::RoleBuildError> {
    let root = repo_root.to_string_lossy().to_string();
    let (py, rs) = kiss::gather_files_by_lang(&[root], None, ignore);
    let py_parsed = crate::analyze_parse::parse_py_files(&py)?;
    let rs_parsed = crate::analyze_parse::parse_rs_files(&rs)?;
    kiss::code_roles::build_source_role_index(&py_parsed, &rs_parsed, &py, &rs)
}

#[cfg(test)]
fn roles_for_changed_paths(
    paths: &[PathBuf],
) -> Result<kiss::code_roles::SourceRoleIndex, kiss::code_roles::RoleBuildError> {
    let py: Vec<_> = paths
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("py")))
        .cloned()
        .collect();
    let rs: Vec<_> = paths
        .iter()
        .filter(|p| kiss::Language::is_rust_path(p))
        .cloned()
        .collect();
    let py_parsed = crate::analyze_parse::parse_py_files(&py)?;
    let rs_parsed = crate::analyze_parse::parse_rs_files(&rs)?;
    kiss::code_roles::build_source_role_index(&py_parsed, &rs_parsed, &py, &rs)
}

pub(crate) fn is_rust_planning_source_path(path: &Path) -> bool {
    kiss::Language::is_rust_path(path) || kiss::rust_llvm_cov_runner::is_rust_cov_cache_input(path)
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
    let rs_candidates: Vec<_> = test_paths
        .iter()
        .filter(|p| p.is_file())
        .filter(|p| kiss::Language::is_rust_path(p))
        .cloned()
        .collect();
    let rs = if rs_candidates.is_empty() {
        Vec::new()
    } else {
        match cargo_workspace_member_manifest_dirs(repo_root) {
            Ok(member_manifest_dirs) => rs_candidates
                .into_iter()
                .filter(|p| is_workspace_rust_selector_file(p, &member_manifest_dirs))
                .collect(),
            Err(_) => rs_candidates,
        }
    };
    if !py.is_empty() {
        if let Some(nodeids) = python_nodeids_from_stored_universe(repo_root, &py) {
            out.python_nodeids.extend(nodeids);
        } else {
            for nodeid in collect_python_nodeids(repo_root, Some(&py), &[])? {
                out.python_nodeids.insert(nodeid);
            }
        }
    }
    if !rs.is_empty() {
        for path in rs {
            let ids =
                crate::test_runner::targets::rust_direct_test_selectors(&path).map_err(|e| {
                    format!("error: kiss test: failed to parse {}: {e}", path.display())
                })?;
            for id in ids {
                out.rust_tests.insert((path.clone(), id));
            }
        }
    }
    Ok(out)
}

fn python_nodeids_from_stored_universe(
    repo_root: &Path,
    py_files: &[PathBuf],
) -> Option<BTreeSet<String>> {
    let selectors = stored_python_universe_selectors(repo_root, &[], PYTHON_COVERAGE_ENV_KEYS)?;
    let mut rels = BTreeSet::new();
    for path in py_files {
        rels.insert(python_repo_relative_path(repo_root, path)?);
    }
    let mut out = BTreeSet::new();
    for selector in selectors {
        let file = selector.split("::").next().unwrap_or(selector.as_str());
        if rels.contains(file) {
            out.insert(selector);
        }
    }
    Some(out)
}

pub(crate) fn require_kiss_test_report_id(
    map: &BTreeMap<String, String>,
    logical: &str,
) -> Result<String, String> {
    map.get(logical).cloned().ok_or_else(|| {
        format!("error: kiss: missing PATH::symbol report id for rust selector `{logical}`")
    })
}

pub(crate) fn rust_report_ids_for_selectors(
    repo_root: &Path,
    selectors: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let map = rust_logical_to_kiss_test_ids(repo_root, &[])?;
    for selector in selectors {
        require_kiss_test_report_id(&map, selector)?;
    }
    Ok(map)
}

pub fn enumerate_workspace_python_selectors(
    repo_root: &Path,
    ignore: &[String],
    pytest_args: &[String],
) -> Result<Vec<String>, String> {
    if !ignore.is_empty() {
        let root = repo_root.to_string_lossy().to_string();
        let (py_files, _rs_files) =
            kiss::gather_files_by_lang(&[root], Some(kiss::Language::Python), ignore);

        let test_paths = py_files
            .into_iter()
            .filter(|path| is_default_pytest_collect_candidate(path))
            .collect::<Vec<_>>();
        return collect_python_nodeids(repo_root, Some(&test_paths), pytest_args);
    }

    let tests_root = repo_root.join("tests");
    if tests_root.is_dir() {
        return collect_python_nodeids(repo_root, Some(&[tests_root]), pytest_args);
    }
    collect_python_nodeids(repo_root, None, pytest_args)
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
        kiss::rust_llvm_cov_runner::placeholder_delegated_runner_fields();
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
        selector_timeout_millis: std::collections::BTreeMap::new(),
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
