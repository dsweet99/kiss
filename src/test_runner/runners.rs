use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(test)]
pub(crate) use super::rust_llvm_cov::rust_llvm_cov_request_from_parts;
pub(crate) use super::rust_llvm_cov::{
    build_cargo_llvm_cov_dry_run_argv, run_rust_llvm_cov_selectors,
};
use kiss::test_refs::{is_in_test_directory, is_test_file};
use kiss::{parse_files, parse_rust_files, rust_test_functions_in, test_functions_in};

#[path = "runners/decision.rs"]
mod decision;
pub(crate) use decision::combined_selectors;

#[path = "runners/python_backer.rs"]
pub(crate) mod python_backer;
#[path = "runners/rust_backer.rs"]
pub(crate) mod rust_backer;

#[path = "runners/rslip.rs"]
mod rslip;
#[cfg(test)]
pub(crate) use rslip::rslip_request_from_parts;
pub(crate) use rslip::run_rslip_selectors;

pub const NO_COVERING_TESTS_MSG: &str = "NO COVERING TESTS";

pub fn py_selector(test_path: &Path, test_id: &str) -> String {
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
    pub(crate) failed: usize,
}

impl SelectorExecutionSummary {
    pub(crate) fn record(
        &mut self,
        status: rpytest_runner::TestStatus,
        cache_hit: bool,
        exit_code: Option<i32>,
    ) {
        self.total += 1;
        if cache_hit {
            self.cache_hits += 1;
        } else {
            self.cache_misses += 1;
        }
        if status == rpytest_runner::TestStatus::Failed {
            self.failed += 1;
            self.exit_code = merge_exit_codes(self.exit_code, exit_code.unwrap_or(1));
        }
    }
}

pub fn partition_changed_paths(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut source = Vec::new();
    let mut test = Vec::new();
    for p in paths {
        let is_py = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("py"));
        let is_rs = kiss::Language::is_rust_path(p);
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

pub fn enumerate_tests_in_changed_files(
    test_paths: &[PathBuf],
) -> Result<BTreeSet<(PathBuf, String)>, String> {
    let mut out = BTreeSet::new();
    let py: Vec<_> = test_paths
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("py")))
        .cloned()
        .collect();
    let rs: Vec<_> = test_paths
        .iter()
        .filter(|p| kiss::Language::is_rust_path(p))
        .cloned()
        .collect();
    if !py.is_empty() {
        let parsed = parse_files(&py).map_err(|e| e.to_string())?;
        for (path, r) in py.iter().zip(parsed) {
            let pf = r.map_err(|e| {
                format!("error: kiss test: failed to parse {}: {e}", path.display())
            })?;
            let ids = test_functions_in(&pf);
            for id in ids {
                out.insert((pf.path.clone(), id));
            }
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
                out.insert((pf.path.clone(), id));
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
    let root = repo_root.to_string_lossy().to_string();
    let (py_files, _rs_files) =
        kiss::gather_files_by_lang(&[root], Some(kiss::Language::Python), ignore);
    let test_files: Vec<_> = py_files
        .into_iter()
        .filter(|path| is_test_file(path) || is_in_test_directory(path))
        .collect();
    let parsed = parse_files(&test_files).map_err(|e| e.to_string())?;
    let mut selectors = BTreeSet::new();
    for (path, result) in test_files.iter().zip(parsed) {
        let pf = result.map_err(|e| {
            format!(
                "error: kiss test: failed to parse Python test file {}: {e}",
                path.display()
            )
        })?;
        for selector in test_functions_in(&pf) {
            selectors.insert(py_selector(&pf.path, &selector));
        }
    }
    Ok(selectors.into_iter().collect())
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
