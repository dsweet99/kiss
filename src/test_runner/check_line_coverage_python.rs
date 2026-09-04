use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use kiss::rpytest_runner::TestStatus;

use super::{
    BackendCoverage, RuntimeCoverageLoadError, backend_from_population,
    classify_python_coverage_file, coverage_error,
};
use crate::test_runner::python_coverage_index::load_python_entry_for_index;
use crate::test_runner::runners::{detect_rslip_versions, rslip_request_from_parts};

pub(super) fn load_python_coverage_from_entries(
    repo_root: &Path,
    pytest_args: &[String],
    population: &crate::test_runner::python_coverage_index::StoredPythonPopulation,
    gate: &kiss::GateConfig,
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
                gate,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| coverage_error("Python", &format!("malformed request ({err})")))?;
    let outcomes = kiss::rslip::load_cached_outcomes_many(&reqs);
    let covered_lines = aggregate_passed_outcomes(repo_root, selectors, outcomes).or_else(|err| {
        match scanned_python_coverage_for_selectors(repo_root, selectors) {
            Ok(Some(lines)) => Ok(lines),
            Ok(None) => Err(err),
            Err(scan_err) => Err(scan_err),
        }
    })?;
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
    outcomes: Vec<Result<Option<kiss::rslip::RslipOutcome>, kiss::rslip::RslipError>>,
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

fn scanned_python_coverage_for_selectors(
    repo_root: &Path,
    selectors: &[String],
) -> Result<Option<BTreeMap<String, BTreeSet<u32>>>, RuntimeCoverageLoadError> {
    let Ok(cache_root) =
        crate::test_runner::python_coverage_index::python_coverage_cache_root(repo_root)
    else {
        return Ok(None);
    };
    let wanted: BTreeSet<_> = selectors.iter().cloned().collect();
    let mut found = BTreeMap::<String, kiss::rslip::LineCoverage>::new();
    for path in crate::test_runner::python_coverage_index::storage::python_coverage_entry_paths(
        &cache_root,
    ) {
        let Some((selector, status, coverage)) = load_python_entry_for_index(&path) else {
            continue;
        };
        if !wanted.contains(&selector) || status != TestStatus::Passed {
            continue;
        }
        found.insert(selector, coverage);
    }
    if selectors
        .iter()
        .any(|selector| !found.contains_key(selector))
    {
        return Ok(None);
    }
    let mut covered_lines = BTreeMap::<String, BTreeSet<u32>>::new();
    for coverage in found.into_values() {
        for (file, lines) in coverage.files {
            let Some(rel) = classify_python_coverage_file(repo_root, &file)? else {
                continue;
            };
            covered_lines.entry(rel).or_default().extend(lines);
        }
    }
    Ok(Some(covered_lines))
}

#[cfg(test)]
mod tests {
    use super::load_python_coverage_from_entries;
    use crate::test_runner::python_coverage_index::StoredPythonPopulation;

    #[test]
    fn missing_repo_is_stale_tool_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("no-such-repo");
        let err = load_python_coverage_from_entries(
            &missing,
            &[],
            &StoredPythonPopulation {
                selectors: vec!["app.py::test_x".into()],
                identity: "id".into(),
            },
            &kiss::GateConfig::default(),
        )
        .expect_err("missing repo cannot resolve python tool identity");
        assert!(
            err.to_string().contains("stale/incompatible tool identity"),
            "{err}"
        );
    }

    #[test]
    fn mismatched_nodeid_is_incomplete_population() {
        use std::time::Duration;

        use kiss::rslip::{CacheStatus, LineCoverage, RslipOutcome};
        use kiss::rpytest_runner::TestStatus;

        let tmp = tempfile::tempdir().unwrap();
        let err = super::aggregate_passed_outcomes(
            tmp.path(),
            &["app.py::test_x".into()],
            vec![Ok(Some(RslipOutcome {
                nodeid: "app.py::test_other".into(),
                status: TestStatus::Passed,
                exit_code: Some(0),
                duration: Duration::from_millis(1),
                coverage: LineCoverage {
                    files: Default::default(),
                },
                cache_status: CacheStatus::Hit,
                stdout: None,
                stderr: None,
            }))],
        )
        .expect_err("selector/nodeid mismatch is incomplete");
        assert!(
            err.to_string().contains("incomplete population"),
            "{err}"
        );
    }
}
