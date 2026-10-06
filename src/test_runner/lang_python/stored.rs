use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::test_runner::lang_iface::{
    ExecutionWitness, GenerationIds, StoredCoverage, WitnessStatus,
};
use crate::test_runner::python_coverage_index::{
    load_current_python_coverage_index, python_coverage_snapshot_generation_id,
    select_python_source_selectors_from_index,
};

/// The Python witness for a run with pytest arguments `extras`: the test records made
/// under the current runner identity, less those whose covered dependencies changed.
pub(super) fn stored_witness(
    repo_root: &Path,
    extras: &[String],
) -> Option<ExecutionWitness> {
    let template = record_request(repo_root, extras)?;
    let identity = kiss::rslip::record_identity_for_request(&template).ok()?;
    let records =
        kiss::test_records::load_records(&kiss::rslip::python_records_dir(&template.source_root));
    let mut selectors = Vec::new();
    let mut statuses = Vec::new();
    let mut durations_ns = Vec::new();
    for record in records
        .into_iter()
        .filter(|record| record.identity == identity)
        .filter(|record| record_holds(&template.source_root, record))
    {
        selectors.push(record.test_id);
        statuses.push(WitnessStatus::from_test_status(record.status));
        durations_ns.push(u64::try_from(record.duration.as_nanos()).ok());
    }
    Some(ExecutionWitness {
        language: "python".into(),
        identity_digest: format!("py:{identity}"),
        selectors,
        durations_ns,
        covered_lines: Default::default(),
        complete: true,
        generation_id: identity,
        raw_statuses: statuses.clone(),
        statuses,
    })
}

pub(super) fn record_identity(repo_root: &Path, extras: &[String]) -> Option<String> {
    let template = record_request(repo_root, extras)?;
    kiss::rslip::record_identity_for_request(&template).ok()
}

/// The current digests of the files `record` covered, keyed as rslip records them.
pub(super) fn current_deps(
    repo_root: &Path,
    record: &kiss::test_records::TestRecord,
) -> Option<std::collections::BTreeMap<String, String>> {
    if record.covered.is_empty() {
        return None;
    }
    let source_root = repo_root.canonicalize().ok()?;
    let coverage = kiss::rslip::LineCoverage {
        files: record.covered.clone(),
    };
    kiss::rslip::covered_file_digests_for(&source_root, &record.test_id, &coverage)
}

/// A request carrying the current runner identity; the node id does not enter it.
fn record_request(repo_root: &Path, extras: &[String]) -> Option<kiss::rslip::RslipRequest> {
    let (python_version, pytest_version) =
        crate::test_runner::runners::detect_rslip_versions(repo_root).ok()?;
    crate::test_runner::runners::rslip_request_from_parts(
        repo_root,
        "identity",
        extras,
        &python_version,
        &pytest_version,
        false,
        &kiss::GateConfig::default(),
    )
    .ok()
}

fn record_holds(source_root: &Path, record: &kiss::test_records::TestRecord) -> bool {
    let current = current_deps(source_root, record);
    kiss::test_records::must_run(
        Some(record.view()),
        &kiss::test_records::Selection {
            identity: &record.identity,
            current_deps: current.as_ref(),
            retry_bad: false,
            needs_duration: false,
        },
    )
    .is_none()
}

pub(super) fn historical_covering_selectors(
    repo_root: &Path,
    keys: &[String],
    abs: &[PathBuf],
) -> BTreeSet<String> {
    let mut selectors = BTreeSet::new();
    if let Some(index) = load_current_python_coverage_index(repo_root) {
        for key in keys {
            if let Some(found) = index.get(key) {
                selectors.extend(found.iter().cloned());
            }
        }
    }
    if let Some(found) = select_python_source_selectors_from_index(repo_root, abs) {
        selectors.extend(found);
    }
    selectors
}

pub(super) fn indexes_path(repo_root: &Path, keys: &[String]) -> bool {
    load_current_python_coverage_index(repo_root)
        .is_some_and(|index| keys.iter().any(|key| index.contains_key(key)))
}

/// Lines covered by the current test records, by repo-relative file.
pub(super) fn stored_coverage(repo_root: &Path) -> StoredCoverage {
    let mut stored = StoredCoverage::default();
    if !kiss::rslip::python_records_dir(repo_root).is_dir() {
        return stored;
    }
    let Some(template) = record_request(repo_root, &[]) else {
        return stored;
    };
    let Ok(identity) = kiss::rslip::record_identity_for_request(&template) else {
        return stored;
    };
    let records =
        kiss::test_records::load_records(&kiss::rslip::python_records_dir(&template.source_root));
    for record in records
        .into_iter()
        .filter(|record| record.identity == identity)
        .filter(|record| record_holds(&template.source_root, record))
    {
        for (file, lines) in record.covered {
            if let Some(rel) =
                crate::test_runner::python_coverage_index::repo_relative_coverage_file(
                    &template.source_root,
                    &file,
                )
                && !kiss::is_python_test_module_path(Path::new(&rel))
            {
                stored.covered.entry(rel).or_default().extend(lines);
            }
        }
    }
    stored
}

pub(super) fn generation_ids(repo_root: &Path) -> GenerationIds {
    GenerationIds {
        witness: None,
        coverage: python_coverage_snapshot_generation_id(repo_root),
    }
}

/// Store a passing record for `selector` under the current runner identity that covers
/// `covered` (repo-relative file to lines).
#[cfg(test)]
pub(crate) fn store_test_record_covering(
    repo_root: &Path,
    selector: &str,
    covered: &std::collections::BTreeMap<String, Vec<u32>>,
) {
    let template = record_request(repo_root, &[]).expect("python record request");
    let identity = kiss::rslip::record_identity_for_request(&template).expect("record identity");
    let root = template.source_root.clone();
    let files = covered
        .iter()
        .map(|(rel, lines)| {
            (
                root.join(rel).to_string_lossy().into_owned(),
                lines.iter().copied().collect(),
            )
        })
        .collect();
    store_record_covering(
        &root,
        &identity,
        selector,
        kiss::rpytest_runner::TestStatus::Passed,
        files,
    );
}

#[cfg(test)]
fn store_record_covering(
    root: &Path,
    identity: &str,
    selector: &str,
    status: kiss::rpytest_runner::TestStatus,
    mut files: std::collections::BTreeMap<String, BTreeSet<u32>>,
) {
    let dir = kiss::rslip::python_records_dir(root);
    std::fs::create_dir_all(&dir).expect("records dir");
    if files.is_empty() {
        files.insert("<frozen importlib._bootstrap>".into(), BTreeSet::from([1]));
    }
    let coverage = kiss::rslip::LineCoverage { files };
    let deps = kiss::rslip::covered_file_digests_for(root, selector, &coverage).unwrap_or_default();
    let record = kiss::test_records::TestRecord {
        schema: kiss::test_records::RECORD_SCHEMA.to_string(),
        language: "python".into(),
        test_id: selector.to_string(),
        identity: identity.to_string(),
        deps,
        status,
        exit_code: Some(i32::from(
            status != kiss::rpytest_runner::TestStatus::Passed,
        )),
        duration: std::time::Duration::from_millis(1),
        covered: coverage.files,
    };
    kiss::test_records::store_record(&dir, &record).expect("store record");
}
