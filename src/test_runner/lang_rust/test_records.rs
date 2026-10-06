use std::path::{Path, PathBuf};

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
    complete: bool,
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

pub(crate) fn load(repo_root: &Path) -> Option<ExecutionWitness> {
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
        covered_lines: Default::default(),
        complete: seeded.complete,
        generation_id: String::new(),
    })
}
