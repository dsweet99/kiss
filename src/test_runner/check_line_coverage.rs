use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use rpytest_runner::TestStatus;

use crate::analyze::line_coverage::RuntimeCoverageSnapshot;
use crate::analyze_cache::fnv1a64;
pub(crate) use crate::test_runner::check_runtime_refresh::ensure_check_runtime_coverage;
use crate::test_runner::python_coverage_index::{
    PYTHON_COVERAGE_ENV_KEYS, python_population_environment_mismatch,
    repo_relative_coverage_file as python_repo_relative_coverage_file,
    repo_relative_path as python_repo_relative_path, stored_python_universe_population,
};
use crate::test_runner::runners::{detect_rslip_versions, rslip_request_from_parts};

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
) -> Result<RuntimeCoverageSnapshot, RuntimeCoverageLoadError> {
    let mut covered_lines = BTreeMap::<String, BTreeSet<u32>>::new();
    let mut identity_parts = Vec::new();
    if required.python {
        let python = load_python_runtime_coverage(repo_root)?;
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

    fn incomplete_population(language: &'static str, problem_selectors: Vec<String>) -> Self {
        Self {
            language,
            reason: "incomplete population".to_string(),
            problem_selectors,
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

/// Command-scoped cov inputs: source/backend validation computed once per `kiss test`.
#[derive(Clone, Debug)]
pub(crate) struct ValidatedCovInputs {
    pub(crate) snapshot: RuntimeCoverageSnapshot,
    #[allow(dead_code)]
    pub(crate) required: RequiredCoverageLanguages,
    pub(crate) python_generation_id: Option<String>,
}

impl ValidatedCovInputs {
    pub(crate) fn from_snapshot(
        required: RequiredCoverageLanguages,
        snapshot: RuntimeCoverageSnapshot,
        repo_root: &Path,
    ) -> Self {
        let python_generation_id = if required.python {
            crate::test_runner::python_coverage_index::try_load_pinned_python_generation_warm(
                repo_root,
            )
            .ok()
            .map(|pinned| pinned.generation_id)
        } else {
            None
        };
        Self {
            required,
            snapshot,
            python_generation_id,
        }
    }
}

/// Pytest `-p` args used when publishing/loading the Python coverage population.
/// Must match `ensure_python_runtime_coverage` / `kiss test` publication.
fn configured_python_coverage_pytest_args() -> Vec<String> {
    kiss::TestSectionConfig::load().pytest_plugin_cli_args()
}

pub(crate) fn load_python_runtime_coverage(
    repo_root: &Path,
) -> Result<BackendCoverage, RuntimeCoverageLoadError> {
    let pytest_args = configured_python_coverage_pytest_args();
    if let Some(coverage) = try_coverage_from_generation(repo_root, &pytest_args)? {
        return Ok(coverage);
    }
    // Opportunistic v1→v2 migrate so warm cov can pin one generation without entry rewalk.
    if crate::test_runner::python_coverage_index::try_migrate_complete_v1_generation(
        repo_root,
        &pytest_args,
        &|path, root| {
            python_repo_relative_coverage_file(root, &path.to_string_lossy()).is_some()
        },
    )
    .ok()
    .flatten()
    .is_some()
    {
        crate::test_runner::python_coverage_index::clear_python_generation_warm_memo();
        if let Some(coverage) = try_coverage_from_generation(repo_root, &pytest_args)? {
            return Ok(coverage);
        }
    }
    let population =
        stored_python_universe_population(repo_root, &pytest_args, PYTHON_COVERAGE_ENV_KEYS)
            .ok_or_else(|| python_population_error(repo_root))?;
    if let Some(covered_lines) =
        crate::test_runner::python_coverage_index::try_load_python_coverage_snapshot(repo_root)
    {
        return Ok(backend_from_population(
            &population.identity,
            &population.selectors,
            covered_lines,
        ));
    }
    load_python_coverage_from_entries(repo_root, &pytest_args, &population)
}

fn try_coverage_from_generation(
    repo_root: &Path,
    pytest_args: &[String],
) -> Result<Option<BackendCoverage>, RuntimeCoverageLoadError> {
    let Ok(pinned) =
        crate::test_runner::python_coverage_index::try_load_pinned_python_generation_warm(repo_root)
    else {
        return Ok(None);
    };
    let exec = crate::test_runner::python_coverage_index::current_python_execution_identity(
        repo_root,
        pytest_args,
    )
    .map_err(|err| coverage_error("Python", &err))?;
    if pinned.plan.base_identity == exec && pinned.complete {
        return Ok(Some(BackendCoverage {
            identity: backend_identity(
                "python",
                &[
                    ("generation".to_string(), pinned.generation_id.clone()),
                    ("selectors".to_string(), pinned.plan.selectors.join("\n")),
                ],
                &pinned.coverage,
            ),
            covered_lines: pinned.coverage,
        }));
    }
    if pinned.plan.base_identity == exec && !pinned.complete {
        let problems = crate::test_runner::python_coverage_index::problem_selectors_from_timings(
            &pinned.timings,
        );
        return Err(RuntimeCoverageLoadError::incomplete_population(
            "Python",
            problems,
        ));
    }
    Ok(None)
}

fn backend_from_population(
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

fn load_python_coverage_from_entries(
    repo_root: &Path,
    pytest_args: &[String],
    population: &crate::test_runner::python_coverage_index::StoredPythonPopulation,
) -> Result<BackendCoverage, RuntimeCoverageLoadError> {
    let selectors = &population.selectors;
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
                pytest_args,
                &python_version,
                &pytest_version,
                false,
        &kiss::GateConfig::load(),
    )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| coverage_error("Python", &format!("malformed request ({err})")))?;
    let outcomes = rslip::load_cached_outcomes_many_trusting_population(&reqs);
    let covered_lines = aggregate_passed_outcomes(repo_root, selectors, outcomes)?;
    let _ = crate::test_runner::python_coverage_index::write_python_coverage_snapshot(
        repo_root,
        &covered_lines,
    );
    Ok(backend_from_population(
        &population.identity,
        selectors,
        covered_lines,
    ))
}

fn aggregate_passed_outcomes(
    repo_root: &Path,
    selectors: &[String],
    outcomes: Vec<Result<Option<rslip::RslipOutcome>, rslip::RslipError>>,
) -> Result<BTreeMap<String, BTreeSet<u32>>, RuntimeCoverageLoadError> {
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
    Ok(covered_lines)
}

fn python_population_error(repo_root: &Path) -> RuntimeCoverageLoadError {
    let pytest_args = configured_python_coverage_pytest_args();
    let Some((recorded, current)) =
        python_population_environment_mismatch(repo_root, &pytest_args, PYTHON_COVERAGE_ENV_KEYS)
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

#[cfg(test)]
#[path = "check_line_coverage_test.rs"]
mod tests;
