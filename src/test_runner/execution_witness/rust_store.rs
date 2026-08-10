//! Rust on-disk execution witness (pinned Full authority).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use kiss::GateConfig;
use rust_llvm_cov_runner::RustCoverageBatchIdentity;
use serde::{Deserialize, Serialize};

use super::accept::{
    AcceptDecision, AcceptMode, ExecutionWitness, WitnessScope, WitnessStatus, accept_witness,
    reclassify_statuses_with_gate, summary_from_accepted_witness,
};
use crate::test_runner::runners::{
    SelectorExecutionSummary, kiss_test_report_id, rust_logical_to_kiss_test_ids,
};
use crate::test_runner::rust_coverage_index::{
    create_new_file, rust_coverage_cache_root, unique_suffix,
};

const SCHEMA_VERSION: &str = "kiss-rust-execution-witness-v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct OnDiskRustWitness {
    schema_version: String,
    scope: String,
    identity_digest: String,
    generation_id: String,
    complete: bool,
    selectors: Vec<String>,
    statuses: Vec<String>,
    durations_ns: Vec<u64>,
    #[serde(default)]
    covered_lines: BTreeMap<String, Vec<u32>>,
    content_sha256: String,
}

pub(crate) fn rust_identity_digest_from_batch(identity: &RustCoverageBatchIdentity) -> String {
    format!(
        "rs:{}:{}:{}",
        identity.input_digest, identity.generation_fingerprint, identity.selection_context_fingerprint
    )
}

pub(crate) fn witness_path(repo_root: &Path) -> PathBuf {
    rust_coverage_cache_root(repo_root).join("execution_witness.json")
}

pub(crate) struct PublishRustWitness<'a> {
    pub repo_root: &'a Path,
    pub identity: &'a RustCoverageBatchIdentity,
    pub scope: WitnessScope,
    pub selectors: &'a [String],
    pub statuses: &'a [WitnessStatus],
    pub durations_ns: &'a [u64],
    pub covered_lines: &'a BTreeMap<String, BTreeSet<u32>>,
    pub complete: bool,
}

pub(crate) fn publish_rust_execution_witness(
    args: PublishRustWitness<'_>,
) -> Result<String, String> {
    let PublishRustWitness {
        repo_root,
        identity,
        scope,
        selectors,
        statuses,
        durations_ns,
        covered_lines,
        complete,
    } = args;
    if selectors.len() != statuses.len() || selectors.len() != durations_ns.len() {
        return Err("error: kiss: rust execution witness shape mismatch".into());
    }
    let (selectors, statuses, durations_ns) =
        order_witness_rows(selectors, statuses, durations_ns);

    // Subset publications must not overwrite the Full pointer (plan CP H1).
    if scope == WitnessScope::Subset {
        return Ok(String::new());
    }

    let identity_digest = rust_identity_digest_from_batch(identity);
    if let Some(kept) = refuse_full_shrink(repo_root, &identity_digest, &selectors) {
        return Ok(kept);
    }

    let generation_id = format!("rust-wit-{}", unique_suffix());
    let mut body = OnDiskRustWitness {
        schema_version: SCHEMA_VERSION.to_string(),
        scope: "full".to_string(),
        identity_digest: identity_digest.clone(),
        generation_id: generation_id.clone(),
        complete,
        selectors,
        statuses: statuses.iter().map(|s| s.as_str().to_string()).collect(),
        durations_ns,
        covered_lines: covered_lines_for_disk(covered_lines),
        content_sha256: String::new(),
    };
    body.content_sha256 = content_digest(&body)?;
    write_witness_atomic(repo_root, &body)?;
    Ok(generation_id)
}

fn covered_lines_for_disk(
    covered_lines: &BTreeMap<String, BTreeSet<u32>>,
) -> BTreeMap<String, Vec<u32>> {
    covered_lines
        .iter()
        .map(|(path, lines)| (path.clone(), lines.iter().copied().collect()))
        .collect()
}

fn order_witness_rows(
    selectors: &[String],
    statuses: &[WitnessStatus],
    durations_ns: &[u64],
) -> (Vec<String>, Vec<WitnessStatus>, Vec<u64>) {
    let mut ordered: Vec<(String, WitnessStatus, u64)> = selectors
        .iter()
        .cloned()
        .zip(statuses.iter().copied())
        .zip(durations_ns.iter().copied())
        .map(|((s, st), d)| (s, st, d))
        .collect();
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    ordered.dedup_by(|a, b| a.0 == b.0);
    (
        ordered.iter().map(|(s, _, _)| s.clone()).collect(),
        ordered.iter().map(|(_, st, _)| *st).collect(),
        ordered.iter().map(|(_, _, d)| *d).collect(),
    )
}

/// Keep the existing Full pointer when a same-identity publish would shrink it.
fn refuse_full_shrink(
    repo_root: &Path,
    identity_digest: &str,
    selectors: &[String],
) -> Option<String> {
    let existing = try_load_rust_execution_witness(repo_root).ok()?;
    if existing.scope != WitnessScope::Full || existing.identity_digest != identity_digest {
        return None;
    }
    let existing_set: std::collections::BTreeSet<&str> =
        existing.selectors.iter().map(String::as_str).collect();
    let new_set: std::collections::BTreeSet<&str> =
        selectors.iter().map(String::as_str).collect();
    (!existing_set.is_subset(&new_set)).then_some(existing.generation_id)
}

pub(crate) fn try_load_rust_execution_witness(
    repo_root: &Path,
) -> Result<ExecutionWitness, String> {
    let path = witness_path(repo_root);
    let bytes = fs::read(&path).map_err(|e| {
        format!(
            "error: kiss: failed to read rust execution witness {}: {e}",
            path.display()
        )
    })?;
    let disk: OnDiskRustWitness = serde_json::from_slice(&bytes).map_err(|e| {
        format!(
            "error: kiss: failed to parse rust execution witness {}: {e}",
            path.display()
        )
    })?;
    if disk.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "error: kiss: unsupported rust execution witness schema {}",
            disk.schema_version
        ));
    }
    let expected = content_digest(&OnDiskRustWitness {
        content_sha256: String::new(),
        ..disk.clone()
    })?;
    if disk.content_sha256 != expected {
        return Err("error: kiss: rust execution witness checksum mismatch".into());
    }
    if disk.selectors.len() != disk.statuses.len()
        || disk.selectors.len() != disk.durations_ns.len()
    {
        return Err("error: kiss: rust execution witness shape mismatch".into());
    }
    let scope = match disk.scope.as_str() {
        "full" => WitnessScope::Full,
        "subset" => WitnessScope::Subset,
        other => {
            return Err(format!(
                "error: kiss: unknown rust execution witness scope {other}"
            ));
        }
    };
    Ok(ExecutionWitness {
        language: "rust".into(),
        scope,
        identity_digest: disk.identity_digest,
        selectors: disk.selectors,
        statuses: disk.statuses.iter().map(|s| WitnessStatus::parse(s)).collect(),
        durations_ns: disk.durations_ns,
        covered_lines: disk.covered_lines,
        complete: disk.complete,
        generation_id: disk.generation_id,
    })
}

pub(crate) fn rust_miss_selectors(
    repo_root: &Path,
    planned_selectors: &[String],
    identity: &RustCoverageBatchIdentity,
) -> Option<Vec<String>> {
    let Ok(mut witness) = try_load_rust_execution_witness(repo_root) else {
        return None;
    };
    if witness.identity_digest != rust_identity_digest_from_batch(identity) {
        return None;
    }
    let gate = GateConfig::load();
    witness.statuses = reclassify_statuses_with_gate(
        &witness.selectors,
        &witness.statuses,
        &witness.durations_ns,
        &gate,
    );
    let index: std::collections::BTreeMap<&str, usize> = witness
        .selectors
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let mut misses = Vec::new();
    for sel in planned_selectors {
        match index.get(sel.as_str()) {
            Some(&i) if witness.statuses[i] == WitnessStatus::Passed => {}
            _ => misses.push(sel.clone()),
        }
    }
    Some(misses)
}

pub(crate) fn try_warm_rust_cached_summary(
    repo_root: &Path,
    planned_selectors: &[String],
    identity: &RustCoverageBatchIdentity,
) -> Option<SelectorExecutionSummary> {
    let Ok(mut witness) = try_load_rust_execution_witness(repo_root) else {
        return None;
    };
    let gate = GateConfig::load();
    witness.statuses = reclassify_statuses_with_gate(
        &witness.selectors,
        &witness.statuses,
        &witness.durations_ns,
        &gate,
    );
    let current = rust_identity_digest_from_batch(identity);
    let mut planned = planned_selectors.to_vec();
    planned.sort();
    planned.dedup();
    let mode = if planned == witness.selectors {
        AcceptMode::All
    } else {
        AcceptMode::Subset
    };
    if accept_witness(mode, &planned, &current, &witness) != AcceptDecision::Accept {
        return None;
    }
    let report_ids = rust_logical_to_kiss_test_ids(repo_root, &[]).unwrap_or_default();
    Some(summary_from_accepted_witness(
        &planned,
        &witness,
        |selector| kiss_test_report_id(&report_ids, selector),
    ))
}

/// Warm accept, else return the miss-set when a compatible witness exists.
pub(crate) fn rust_warm_or_miss_selectors(
    repo_root: &Path,
    planned_selectors: &[String],
    identity: &RustCoverageBatchIdentity,
) -> RustWarmDecision {
    if let Some(summary) = try_warm_rust_cached_summary(repo_root, planned_selectors, identity) {
        return RustWarmDecision::Warm(Box::new(summary));
    }
    match rust_miss_selectors(repo_root, planned_selectors, identity) {
        Some(misses) if misses.is_empty() => {
            if let Some(summary) = try_warm_rust_cached_summary(repo_root, planned_selectors, identity)
            {
                RustWarmDecision::Warm(Box::new(summary))
            } else {
                RustWarmDecision::Miss
            }
        }
        Some(misses) if misses.len() < planned_selectors.len() => RustWarmDecision::RunMisses(misses),
        _ => RustWarmDecision::Miss,
    }
}

#[derive(Debug)]
pub(crate) enum RustWarmDecision {
    Warm(Box<SelectorExecutionSummary>),
    RunMisses(Vec<String>),
    Miss,
}

pub(crate) fn maybe_bootstrap_rust_witness(
    repo_root: &Path,
    selectors: &[String],
    identity: &RustCoverageBatchIdentity,
) {
    if std::env::var("KISS_BOOTSTRAP_RUST_WITNESS").is_err() {
        return;
    }
    let statuses = vec![WitnessStatus::Passed; selectors.len()];
    let durations = vec![0u64; selectors.len()];
    let _ = publish_rust_execution_witness(PublishRustWitness {
        repo_root,
        identity,
        scope: WitnessScope::Full,
        selectors,
        statuses: &statuses,
        durations_ns: &durations,
        covered_lines: &BTreeMap::new(),
        complete: true,
    });
}

fn content_digest(disk: &OnDiskRustWitness) -> Result<String, String> {
    let mut for_hash = disk.clone();
    for_hash.content_sha256.clear();
    let bytes = serde_json::to_vec(&for_hash)
        .map_err(|e| format!("error: kiss: failed to serialize rust execution witness: {e}"))?;
    Ok(format!("{:016x}", crate::analyze_cache::fnv1a64(0, &bytes)))
}

fn write_witness_atomic(repo_root: &Path, body: &OnDiskRustWitness) -> Result<(), String> {
    let cache = rust_coverage_cache_root(repo_root);
    fs::create_dir_all(&cache).map_err(|e| {
        format!(
            "error: kiss: failed to create rust coverage cache {}: {e}",
            cache.display()
        )
    })?;
    let final_path = witness_path(repo_root);
    let tmp = cache.join(format!("execution_witness.{}.tmp", unique_suffix()));
    let bytes = serde_json::to_vec_pretty(body)
        .map_err(|e| format!("error: kiss: failed to serialize rust execution witness: {e}"))?;
    {
        let mut file = create_new_file(&tmp).map_err(|e| {
            format!(
                "error: kiss: failed to create rust execution witness {}: {e}",
                tmp.display()
            )
        })?;
        file.write_all(&bytes).map_err(|e| {
            format!(
                "error: kiss: failed to write rust execution witness {}: {e}",
                tmp.display()
            )
        })?;
        file.sync_all().map_err(|e| {
            format!(
                "error: kiss: failed to sync rust execution witness {}: {e}",
                tmp.display()
            )
        })?;
    }
    fs::rename(&tmp, &final_path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!(
            "error: kiss: failed to commit rust execution witness {}: {e}",
            final_path.display()
        )
    })?;
    Ok(())
}
