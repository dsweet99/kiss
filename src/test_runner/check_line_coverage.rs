use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use rpytest_runner::TestStatus;

use crate::analyze::line_coverage::RuntimeCoverageSnapshot;
pub(crate) use crate::test_runner::check_runtime_refresh::ensure_check_runtime_coverage;
use crate::test_runner::python_coverage_index::{
    PYTHON_COVERAGE_ENV_KEYS, python_population_environment_mismatch,
    repo_relative_coverage_file as python_repo_relative_coverage_file,
    repo_relative_path as python_repo_relative_path, stored_python_universe_population,
};
use crate::test_runner::runners::{detect_rslip_versions, rslip_request_from_parts};
use crate::test_runner::rust_coverage_index::{
    current_rust_coverage_batch_identity,
    repo_relative_coverage_file as rust_repo_relative_coverage_file, rust_coverage_cache_root,
};

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
) -> Result<RuntimeCoverageSnapshot, RuntimeCoverageLoadError> {
    let mut covered_lines = BTreeMap::<String, BTreeSet<u32>>::new();
    let mut identity_parts = Vec::new();
    if required.python {
        let python = load_python_runtime_coverage(repo_root)?;
        identity_parts.push(("python".to_string(), python.identity));
        merge_lines(&mut covered_lines, python.covered_lines);
    }
    if required.rust {
        let rust = load_rust_runtime_coverage(repo_root, ignore)?;
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
}

impl RuntimeCoverageLoadError {
    fn new(language: &'static str, reason: impl Into<String>) -> Self {
        Self {
            language,
            reason: reason.into(),
        }
    }
}

impl fmt::Display for RuntimeCoverageLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "error: kiss cov: {} runtime line coverage is {}.",
            self.language, self.reason
        )
    }
}

pub(super) struct BackendCoverage {
    identity: String,
    covered_lines: BTreeMap<String, BTreeSet<u32>>,
}

pub(super) fn load_python_runtime_coverage(
    repo_root: &Path,
) -> Result<BackendCoverage, RuntimeCoverageLoadError> {
    let population = stored_python_universe_population(repo_root, &[], PYTHON_COVERAGE_ENV_KEYS)
        .ok_or_else(|| python_population_error(repo_root))?;
    let selectors = population.selectors;
    let (python_version, pytest_version) = detect_rslip_versions(repo_root).map_err(|err| {
        coverage_error(
            "Python",
            &format!("stale/incompatible tool identity ({err})"),
        )
    })?;
    let reqs = selectors
        .iter()
        .map(|selector| {
            rslip_request_from_parts(
                repo_root,
                selector,
                &[],
                &python_version,
                &pytest_version,
                false,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| coverage_error("Python", &format!("malformed request ({err})")))?;
    let outcomes = rslip::load_cached_outcomes_many(&reqs);
    let mut covered_lines = BTreeMap::<String, BTreeSet<u32>>::new();
    for (selector, outcome) in selectors.iter().zip(outcomes) {
        let outcome = outcome
            .map_err(|err| coverage_error("Python", &format!("malformed cache entry ({err:?})")))?
            .ok_or_else(|| coverage_error("Python", "incomplete population"))?;
        if outcome.nodeid != *selector || outcome.status != TestStatus::Passed {
            return Err(coverage_error("Python", "incomplete population"));
        }
        for (file, lines) in outcome.coverage.files {
            let Some(rel) = classify_python_coverage_file(repo_root, &file)? else {
                continue;
            };
            covered_lines.entry(rel).or_default().extend(lines);
        }
    }
    let identity = backend_identity(
        "python",
        &[
            ("population".to_string(), population.identity),
            ("selectors".to_string(), selectors.join("\n")),
        ],
        &covered_lines,
    );
    Ok(BackendCoverage {
        identity,
        covered_lines,
    })
}

fn python_population_error(repo_root: &Path) -> RuntimeCoverageLoadError {
    let Some((recorded, current)) =
        python_population_environment_mismatch(repo_root, &[], PYTHON_COVERAGE_ENV_KEYS)
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

fn classify_python_coverage_file(
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

pub(super) fn load_rust_runtime_coverage(
    repo_root: &Path,
    ignore: &[String],
) -> Result<BackendCoverage, RuntimeCoverageLoadError> {
    let identity = current_rust_coverage_batch_identity(repo_root, &[]).map_err(|err| {
        coverage_error("Rust", &format!("stale/incompatible tool identity ({err})"))
    })?;
    let cache_root = rust_coverage_cache_root(repo_root);
    // Prefer aggregate hit without re-enumerating workspace selectors. Source digests in
    // `identity` already invalidate when test/source files change.
    if let Some(snapshot) = rust_llvm_cov_runner::load_current_check_aggregate_snapshot(
        &cache_root,
        repo_root,
        &identity,
        None,
    ) {
        return Ok(BackendCoverage {
            identity: snapshot.identity,
            covered_lines: snapshot.covered_lines,
        });
    }
    let selectors =
        crate::test_runner::runners::enumerate_workspace_rust_selectors(repo_root, ignore)
            .map_err(|err| coverage_error("Rust", &format!("selector discovery failed ({err})")))?;
    if let Some(snapshot) = rust_llvm_cov_runner::load_current_check_aggregate_snapshot(
        &cache_root,
        repo_root,
        &identity,
        Some(&selectors),
    ) {
        return Ok(BackendCoverage {
            identity: snapshot.identity,
            covered_lines: snapshot.covered_lines,
        });
    }
    let snapshot = rust_llvm_cov_runner::load_current_generation_coverage_snapshot(
        &cache_root,
        repo_root,
        &identity,
        Some(&selectors),
    )
    .ok_or_else(|| {
        coverage_error(
            "Rust",
            "missing, stale/incompatible, incomplete, or malformed population",
        )
    })?;
    let mut covered_lines = BTreeMap::<String, BTreeSet<u32>>::new();
    for (file, lines) in snapshot.covered_lines {
        let rel = rust_repo_relative_coverage_file(repo_root, &file)
            .ok_or_else(|| coverage_error("Rust", "malformed out-of-repository path"))?;
        covered_lines.entry(rel).or_default().extend(lines);
    }
    Ok(BackendCoverage {
        identity: snapshot.identity,
        covered_lines,
    })
}

fn coverage_error(language: &'static str, reason: &str) -> RuntimeCoverageLoadError {
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

fn combined_identity(
    parts: &[(String, String)],
    covered_lines: &BTreeMap<String, BTreeSet<u32>>,
) -> String {
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, b"kiss-runtime-line-coverage-v1");
    for (key, value) in parts {
        h = fnv1a64(h, key.as_bytes());
        h = fnv1a64(h, &[0]);
        h = fnv1a64(h, value.as_bytes());
        h = fnv1a64(h, &[0]);
    }
    for (file, lines) in covered_lines {
        h = fnv1a64(h, file.as_bytes());
        h = fnv1a64(h, &[0]);
        for line in lines {
            h = fnv1a64(h, line.to_le_bytes().as_slice());
        }
        h = fnv1a64(h, &[0]);
    }
    format!("{h:016x}")
}

fn fnv1a64(h: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0100_0000_01b3;
    bytes
        .iter()
        .fold(h, |acc, byte| (acc ^ u64::from(*byte)).wrapping_mul(PRIME))
}

#[cfg(test)]
#[path = "check_line_coverage_test.rs"]
mod tests;
