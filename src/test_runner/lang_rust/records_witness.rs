use std::collections::BTreeSet;
use std::path::Path;

use crate::test_runner::lang_iface::{ExecutionWitness, WitnessStatus};

const NO_RECORDS: &str = "error: kiss: no rust test records";

/// The witness identity for Rust records stored under `record_identity`.
pub(crate) fn rust_witness_identity(record_identity: &str) -> String {
    format!("rs:{record_identity}")
}

/// The Rust witness: the test records made under the current identity whose
/// dependencies still match, limited to tests that still exist.
pub(crate) fn try_load_rust_execution_witness(
    repo_root: &Path,
) -> Result<ExecutionWitness, String> {
    #[cfg(test)]
    if let Some(witness) = super::test_records::load(repo_root) {
        return Ok(witness);
    }
    if !kiss::test_records::records_dir(repo_root, "rust").is_dir() {
        return Err(NO_RECORDS.into());
    }
    let gate = kiss::GateConfig::load_for_repo(repo_root);
    let (identity, mut records) = super::nextest::holding_records(repo_root, &[], &gate)?;
    if let Some(known) = known_selectors(repo_root) {
        records.retain(|record| known.contains(&record.test_id));
    }
    if records.is_empty() {
        return Err(NO_RECORDS.into());
    }
    records.sort_by(|a, b| a.test_id.cmp(&b.test_id));
    let statuses: Vec<WitnessStatus> = records
        .iter()
        .map(|record| WitnessStatus::from_test_status(record.status))
        .collect();
    Ok(ExecutionWitness {
        language: "rust".into(),
        identity_digest: rust_witness_identity(&identity),
        generation_id: records_digest(&identity, &records),
        selectors: records
            .iter()
            .map(|record| record.test_id.clone())
            .collect(),
        durations_ns: records
            .iter()
            .map(|record| u64::try_from(record.duration.as_nanos()).ok())
            .collect(),
        covered_lines: Default::default(),
        complete: statuses
            .iter()
            .all(|status| *status == WitnessStatus::Passed),
        raw_statuses: statuses.clone(),
        statuses,
    })
}

pub(super) fn known_selectors(repo_root: &Path) -> Option<BTreeSet<String>> {
    let known = crate::test_runner::workspace_selector_cache::cached_rust_selectors_if_rust_fingerprint_current(
        repo_root,
    )?;
    (!known.is_empty()).then(|| known.into_iter().collect())
}

fn records_digest(identity: &str, records: &[kiss::test_records::TestRecord]) -> String {
    let mut text = identity.to_string();
    for record in records {
        text.push_str(&format!(
            "\n{}\t{:?}\t{}",
            record.test_id,
            record.status,
            record.duration.as_nanos()
        ));
    }
    format!(
        "{:016x}",
        crate::analyze_cache::fnv1a64(0xcbf2_9ce4_8422_2325, text.as_bytes())
    )
}

/// The planned tests with no holding record, given the witness built from those records.
pub(crate) fn record_misses(planned: &[String], witness: Option<&ExecutionWitness>) -> Vec<String> {
    let held: BTreeSet<&str> = witness
        .map(|witness| witness.selectors.iter().map(String::as_str).collect())
        .unwrap_or_default();
    planned
        .iter()
        .filter(|selector| !held.contains(selector.as_str()))
        .cloned()
        .collect()
}
