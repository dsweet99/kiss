use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::analyze::line_coverage::RuntimeCoverageSnapshot;
pub(crate) use crate::test_runner::check_runtime_refresh::ensure_check_runtime_coverage;
use crate::test_runner::python_coverage_index::{
    PYTHON_COVERAGE_ENV_KEYS, python_population_environment_mismatch,
    repo_relative_coverage_file as python_repo_relative_coverage_file,
    repo_relative_path as python_repo_relative_path, stored_python_universe_population,
};
#[path = "check_line_coverage_python.rs"]
mod check_line_coverage_python;
use check_line_coverage_python::load_python_coverage_from_entries;
#[path = "check_line_coverage_rust.rs"]
mod check_line_coverage_rust;
pub(crate) use check_line_coverage_rust::load_rust_runtime_coverage;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RequiredCoverageLanguages {
    pub(crate) python: bool,
    pub(crate) rust: bool,
}

pub(crate) fn repository_root_for_universe(universe: &Path) -> PathBuf {
    let start = universe
        .canonicalize()
        .unwrap_or_else(|_| universe.to_path_buf());
    let start_dir = if start.is_file() {
        start.parent().unwrap_or(&start).to_path_buf()
    } else {
        start.clone()
    };
    let mut cursor = start_dir.as_path();
    loop {
        if cursor.join(".git").exists() {
            return cursor.to_path_buf();
        }
        let Some(parent) = cursor.parent() else {
            return start_dir;
        };
        cursor = parent;
    }
}

pub(crate) fn load_check_runtime_coverage(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
    gate: &kiss::GateConfig,
    pytest_args: &[String],
) -> Result<RuntimeCoverageSnapshot, RuntimeCoverageLoadError> {
    let mut covered_lines = BTreeMap::<String, BTreeSet<u32>>::new();
    let mut identity_parts = Vec::new();
    if required.python {
        let python = load_python_runtime_coverage(repo_root, pytest_args, gate)?;
        identity_parts.push(("python".to_string(), python.identity));
        merge_lines(&mut covered_lines, python.covered_lines);
    }
    if required.rust {
        let rust = load_rust_runtime_coverage(repo_root, ignore, gate)?;
        identity_parts.push(("rust".to_string(), rust.identity));
        merge_lines(&mut covered_lines, rust.covered_lines);
    }
    identity_parts.sort();
    let identity = combined_identity(&identity_parts, &covered_lines);
    Ok(RuntimeCoverageSnapshot {
        identity,
        covered_lines,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeCoverageLoadError {
    pub(crate) language: &'static str,
    pub(crate) reason: String,
    pub(crate) problem_selectors: Vec<String>,
}

impl RuntimeCoverageLoadError {
    fn new(language: &'static str, reason: impl Into<String>) -> Self {
        Self {
            language,
            reason: reason.into(),
            problem_selectors: Vec::new(),
        }
    }
}

impl fmt::Display for RuntimeCoverageLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "error: kiss test: {} runtime line coverage is {}.",
            self.language, self.reason
        )
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BackendCoverage {
    pub(crate) identity: String,
    pub(crate) covered_lines: BTreeMap<String, BTreeSet<u32>>,
}

#[derive(Clone, Debug)]
pub(crate) struct ValidatedCovInputs {
    pub(crate) snapshot: RuntimeCoverageSnapshot,
    #[allow(dead_code)]
    pub(crate) required: RequiredCoverageLanguages,
}

impl ValidatedCovInputs {
    pub(crate) fn from_snapshot(
        required: RequiredCoverageLanguages,
        snapshot: RuntimeCoverageSnapshot,
        repo_root: &Path,
    ) -> Self {
        let _ = repo_root;
        Self { required, snapshot }
    }
}

pub(crate) fn load_python_runtime_coverage(
    repo_root: &Path,
    pytest_args: &[String],
    gate: &kiss::GateConfig,
) -> Result<BackendCoverage, RuntimeCoverageLoadError> {
    use crate::test_runner::lang_iface::KernelRules;
    let recorded = crate::test_runner::lang_python::PythonKernelRules.stored_coverage(repo_root);
    if !recorded.covered.is_empty() {
        return Ok(BackendCoverage {
            identity: backend_identity("python", &[], &recorded.covered),
            covered_lines: recorded.covered,
        });
    }
    let population =
        stored_python_universe_population(repo_root, pytest_args, PYTHON_COVERAGE_ENV_KEYS)
            .ok_or_else(|| python_population_error(repo_root, pytest_args))?;
    if let Some(covered_lines) =
        crate::test_runner::python_coverage_index::try_load_python_coverage_snapshot(repo_root)
    {
        return Ok(backend_from_population(
            &population.identity,
            &population.selectors,
            covered_lines,
        ));
    }
    load_python_coverage_from_entries(repo_root, pytest_args, &population, gate)
}

pub(super) fn backend_from_population(
    population_identity: &str,
    selectors: &[String],
    covered_lines: BTreeMap<String, BTreeSet<u32>>,
) -> BackendCoverage {
    BackendCoverage {
        identity: backend_identity(
            "python",
            &[
                ("population".to_string(), population_identity.to_string()),
                ("selectors".to_string(), selectors.join("\n")),
            ],
            &covered_lines,
        ),
        covered_lines,
    }
}

fn python_population_error(repo_root: &Path, pytest_args: &[String]) -> RuntimeCoverageLoadError {
    let Some((recorded, current)) =
        python_population_environment_mismatch(repo_root, pytest_args, PYTHON_COVERAGE_ENV_KEYS)
    else {
        return coverage_error("Python", "missing or stale/incompatible population");
    };
    coverage_error(
        "Python",
        &format!(
            "population was recorded with {} but the current environment has {}",
            format_python_coverage_env(&recorded),
            format_python_coverage_env(&current),
        ),
    )
}

fn format_python_coverage_env(env: &BTreeMap<String, String>) -> String {
    env.get("PYTHONPATH")
        .map(|value| format!("PYTHONPATH={value:?}"))
        .unwrap_or_else(|| "PYTHONPATH unset".to_string())
}

pub(super) fn classify_python_coverage_file(
    repo_root: &Path,
    file: &str,
) -> Result<Option<String>, RuntimeCoverageLoadError> {
    let path = Path::new(file);
    if !path.is_absolute()
        && !file.starts_with('<')
        && let Some(rel) = python_repo_relative_path(repo_root, path)
    {
        if rel.ends_with(".py") && !rel.starts_with(".kiss/") {
            return Err(coverage_error("Python", "malformed relative source path"));
        }
        return Ok(None);
    }
    if let Some(rel) = python_repo_relative_coverage_file(repo_root, file) {
        return Ok(Some(rel));
    }
    if python_repo_relative_path(repo_root, path).is_some() {
        return Ok(None);
    }
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
    {
        return Err(coverage_error("Python", "malformed out-of-repository path"));
    }
    Ok(None)
}

pub(super) fn coverage_error(language: &'static str, reason: &str) -> RuntimeCoverageLoadError {
    RuntimeCoverageLoadError::new(language, reason)
}

fn merge_lines(
    target: &mut BTreeMap<String, BTreeSet<u32>>,
    source: BTreeMap<String, BTreeSet<u32>>,
) {
    for (file, lines) in source {
        target.entry(file).or_default().extend(lines);
    }
}

fn backend_identity(
    language: &str,
    identity_parts: &[(String, String)],
    covered_lines: &BTreeMap<String, BTreeSet<u32>>,
) -> String {
    let mut parts = identity_parts
        .iter()
        .map(|(key, value)| (format!("{language}:{key}"), value.clone()))
        .collect::<Vec<_>>();
    for (file, lines) in covered_lines {
        parts.push((
            format!("{language}:file:{file}"),
            lines
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ));
    }
    combined_identity(&parts, covered_lines)
}

#[path = "check_line_coverage_identity.rs"]
mod identity;
use identity::combined_identity;

#[cfg(test)]
#[path = "check_line_coverage_test.rs"]
mod tests;
