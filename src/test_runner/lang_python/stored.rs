#![cfg_attr(not(test), allow(dead_code))]
use std::path::Path;

use crate::test_runner::lang_iface::{ExecutionWitness, GenerationIds, WitnessStatus};

pub(crate) fn stored_witness(repo_root: &Path, extras: &[String]) -> Option<ExecutionWitness> {
    let identity = super::records::record_identity(repo_root, extras).ok()?;
    let deps =
        super::records::CurrentDeps::new(repo_root, &kiss::GateConfig::load_for_repo(repo_root))
            .ok()?;
    let records =
        kiss::test_records::load_records(&kiss::test_records::records_dir(repo_root, "python"));
    let mut selectors = Vec::new();
    let mut statuses = Vec::new();
    let mut durations_ns = Vec::new();
    for record in records
        .into_iter()
        .filter(|record| record.identity == identity)
        .filter(|record| record_holds(&deps, record))
    {
        selectors.push(record.test_id);
        statuses.push(WitnessStatus::from_test_status(record.status));
        durations_ns.push(u64::try_from(record.duration.as_nanos()).ok());
    }
    Some(ExecutionWitness {
        language: kiss::Language::Python,
        identity_digest: python_witness_identity(&identity),
        selectors,
        durations_ns,
        complete: true,
        generation_id: identity,
        raw_statuses: statuses.clone(),
        statuses,
    })
}

pub(super) fn python_witness_identity(record_identity: &str) -> String {
    format!("py:{record_identity}")
}

fn record_holds(
    deps: &super::records::CurrentDeps,
    record: &kiss::test_records::TestRecord,
) -> bool {
    let current = deps.of(record);
    kiss::test_records::must_run(
        Some(record.view()),
        &kiss::test_records::Selection {
            identity: &record.identity,
            current_deps: Some(&current),
            retry_bad: false,
            needs_duration: false,
        },
    )
    .is_none()
}

pub(super) fn generation_ids(_repo_root: &Path) -> GenerationIds {
    GenerationIds { witness: None }
}
