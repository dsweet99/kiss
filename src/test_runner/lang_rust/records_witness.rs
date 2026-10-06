use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::test_runner::lang_iface::{ExecutionWitness, WitnessStatus};
use crate::test_runner::rust_coverage_index::{
    current_rust_coverage_batch_identity, resolved_rust_batch_request_parts,
};

pub(crate) use super::witness_identity::rust_identity_digest_from_batch;

const NO_RECORDS: &str = "error: kiss: no rust test records";

/// The Rust witness: the test records made under the current toolchain identity whose
/// dependency digests still match the sources, limited to tests that still exist.
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
    let (req, tools) = resolved_rust_batch_request_parts(repo_root, &[])?;
    let identity = kiss::rust_llvm_cov_runner::rust_record_identity(&req, &tools)
        .map_err(|err| format!("error: kiss: rust record identity: {err}"))?;
    let batch = current_rust_coverage_batch_identity(repo_root, &[])?;
    let mut records = kiss::rust_llvm_cov_runner::rust_records_holding(repo_root, &identity);
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
        identity_digest: rust_identity_digest_from_batch(&batch),
        generation_id: records_digest(&identity, &records),
        selectors: records
            .iter()
            .map(|record| record.test_id.clone())
            .collect(),
        durations_ns: records
            .iter()
            .map(|record| u64::try_from(record.duration.as_nanos()).ok())
            .collect(),
        covered_lines: repo_relative_covered_lines(repo_root, &records),
        complete: statuses
            .iter()
            .all(|status| *status == WitnessStatus::Passed),
        raw_statuses: statuses.clone(),
        statuses,
    })
}

fn repo_relative_covered_lines(
    repo_root: &Path,
    records: &[kiss::test_records::TestRecord],
) -> BTreeMap<String, Vec<u32>> {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let mut covered: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    for record in records {
        for (file, lines) in &record.covered {
            let Ok(rel) = Path::new(file).strip_prefix(&root) else {
                continue;
            };
            covered
                .entry(rel.to_string_lossy().replace('\\', "/"))
                .or_default()
                .extend(lines.iter().copied());
        }
    }
    covered
        .into_iter()
        .map(|(file, lines)| (file, lines.into_iter().collect()))
        .collect()
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

/// The `planned` tests whose records do not hold for a run with test arguments `test_args`.
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
