use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity;
use serde::{Deserialize, Serialize};

use crate::test_runner::lang_iface::{ExecutionWitness, WitnessStatus};

fn records_path(repo_root: &Path) -> PathBuf {
    crate::test_runner::test_state_dir(repo_root).join("rust-test-records.json")
}

#[derive(Serialize, Deserialize)]
struct Seeded {
    identity_digest: String,
    selectors: Vec<String>,
    statuses: Vec<String>,
    durations_ns: Vec<Option<u64>>,
    covered_lines: BTreeMap<String, Vec<u32>>,
    complete: bool,
}

/// Rows a test supplies in place of the Rust records, which need a real cargo build.
pub(crate) struct SeedRustWitness<'a> {
    pub repo_root: &'a Path,
    pub identity: &'a RustCoverageBatchIdentity,
    pub selectors: &'a [String],
    pub statuses: &'a [WitnessStatus],
    pub durations_ns: &'a [Option<u64>],
    pub covered_lines: &'a BTreeMap<String, BTreeSet<u32>>,
    pub complete: bool,
}

pub(crate) fn seed_rust_witness(seed: SeedRustWitness<'_>) -> Result<(), String> {
    write(
        seed.repo_root,
        &Seeded {
            identity_digest: super::rust_identity_digest_from_batch(seed.identity),
            selectors: seed.selectors.to_vec(),
            statuses: seed
                .statuses
                .iter()
                .map(|s| s.as_str().to_string())
                .collect(),
            durations_ns: seed.durations_ns.to_vec(),
            covered_lines: seed
                .covered_lines
                .iter()
                .map(|(path, lines)| (path.clone(), lines.iter().copied().collect()))
                .collect(),
            complete: seed.complete,
        },
    );
    Ok(())
}

/// Seeds passing-or-failing rows with a one-nanosecond duration and no identity.
pub(crate) fn store(repo_root: &Path, rows: &[(String, WitnessStatus)]) {
    write(
        repo_root,
        &Seeded {
            identity_digest: String::new(),
            selectors: rows.iter().map(|(selector, _)| selector.clone()).collect(),
            statuses: rows
                .iter()
                .map(|(_, status)| status.as_str().to_string())
                .collect(),
            durations_ns: vec![Some(1); rows.len()],
            covered_lines: BTreeMap::new(),
            complete: true,
        },
    );
}

fn write(repo_root: &Path, seeded: &Seeded) {
    let path = records_path(repo_root);
    std::fs::create_dir_all(path.parent().expect("records dir")).expect("create records dir");
    std::fs::write(path, serde_json::to_vec(seeded).expect("records json"))
        .expect("write rust test records");
}

pub(super) fn load(repo_root: &Path) -> Option<ExecutionWitness> {
    let bytes = std::fs::read(records_path(repo_root)).ok()?;
    let seeded: Seeded = serde_json::from_slice(&bytes).ok()?;
    let statuses: Vec<WitnessStatus> = seeded
        .statuses
        .iter()
        .map(|raw| WitnessStatus::parse(raw))
        .collect();
    Some(ExecutionWitness {
        language: "rust".into(),
        identity_digest: seeded.identity_digest,
        selectors: seeded.selectors,
        durations_ns: seeded.durations_ns,
        raw_statuses: statuses.clone(),
        statuses,
        covered_lines: seeded.covered_lines,
        complete: seeded.complete,
        generation_id: String::new(),
    })
}
