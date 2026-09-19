use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::{
    OnDiskRustWitness, SCHEMA_VERSION, content_digest, prune_removed_rust_witness_selectors,
    rust_source_identity_covers, witness_path,
};
use crate::test_runner::lang_iface::{ExecutionWitness, WitnessScope, WitnessStatus};
use crate::test_runner::rust_coverage_index::rust_coverage_cache_root;

fn merge_rust_witness_layers(
    layers: impl IntoIterator<Item = ExecutionWitness>,
) -> Option<ExecutionWitness> {
    let layers: Vec<ExecutionWitness> = layers.into_iter().collect();
    let last = layers.last()?.clone();
    let mut selectors = std::collections::BTreeSet::new();
    for layer in &layers {
        selectors.extend(layer.selectors.iter().cloned());
    }
    let selectors: Vec<String> = selectors.into_iter().collect();
    let mut statuses = Vec::with_capacity(selectors.len());
    let mut durations_ns = Vec::with_capacity(selectors.len());
    let mut raw_statuses = Vec::with_capacity(selectors.len());
    for selector in &selectors {
        let row = layers
            .iter()
            .rev()
            .find_map(|layer| rust_witness_row(layer, selector));
        let (status, duration, raw) = row?;
        statuses.push(status);
        durations_ns.push(duration);
        raw_statuses.push(raw);
    }
    let mut covered_lines: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for layer in &layers {
        for (path, lines) in &layer.covered_lines {
            covered_lines
                .entry(path.clone())
                .or_default()
                .extend(lines.iter().copied());
        }
    }
    for lines in covered_lines.values_mut() {
        lines.sort_unstable();
        lines.dedup();
    }
    Some(ExecutionWitness {
        language: last.language,
        scope: if layers
            .iter()
            .any(|layer| layer.scope == WitnessScope::Full)
        {
            WitnessScope::Full
        } else {
            last.scope
        },
        identity_digest: last.identity_digest,
        selectors,
        statuses: statuses.clone(),
        durations_ns,
        covered_lines,
        complete: statuses
            .iter()
            .all(|status| *status == WitnessStatus::Passed),
        generation_id: last.generation_id,
        raw_statuses,
    })
}

fn rust_witness_row(
    witness: &ExecutionWitness,
    selector: &str,
) -> Option<(WitnessStatus, Option<u64>, WitnessStatus)> {
    let i = witness
        .selectors
        .iter()
        .position(|stored| stored == selector)?;
    let status = *witness.statuses.get(i)?;
    let duration = witness.durations_ns.get(i).copied().unwrap_or(None);
    let raw = witness.raw_statuses.get(i).copied().unwrap_or(status);
    Some((status, duration, raw))
}

pub(crate) fn try_load_rust_execution_witness(
    repo_root: &Path,
) -> Result<ExecutionWitness, String> {
    let cache_root = rust_coverage_cache_root(repo_root);
    let memo_path = witness_path(repo_root);
    if let Some(mut witness) = super::super::witness_memo::memo_witness(repo_root, &memo_path) {
        prune_removed_rust_witness_selectors(repo_root, &mut witness)?;
        return Ok(witness);
    }
    if crate::test_runner::execution_generation::read_pointer(&cache_root)?.is_some() {
        let generation = super::super::generation_publish::load_full_generation_witness(repo_root)?;
        let disk = load_witness_from_disk(repo_root).ok().filter(|disk| {
            rust_source_identity_covers(&disk.identity_digest, &generation.identity_digest)
                || disk.identity_digest == generation.identity_digest
        });
        let mut witness = merge_rust_witness_layers(disk.into_iter().chain(std::iter::once(generation)))
            .ok_or_else(|| "error: kiss: rust execution witness merge empty".to_string())?;
        super::super::witness_memo::stash_published_witness(
            repo_root,
            &memo_path,
            witness.clone(),
        );
        prune_removed_rust_witness_selectors(repo_root, &mut witness)?;
        return Ok(witness);
    }
    load_witness_from_disk(repo_root)
}

pub(crate) fn load_witness_from_disk(repo_root: &Path) -> Result<ExecutionWitness, String> {
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
    let mut witness = ExecutionWitness {
        language: "rust".into(),
        scope,
        identity_digest: disk.identity_digest,
        selectors: disk.selectors,
        statuses: disk
            .statuses
            .iter()
            .map(|s| WitnessStatus::parse(s))
            .collect(),
        durations_ns: disk.durations_ns,
        covered_lines: disk.covered_lines,
        complete: disk.complete,
        generation_id: disk.generation_id,
        raw_statuses: disk
            .statuses
            .iter()
            .map(|s| WitnessStatus::parse(s))
            .collect(),
    };
    prune_removed_rust_witness_selectors(repo_root, &mut witness)?;
    Ok(witness)
}
