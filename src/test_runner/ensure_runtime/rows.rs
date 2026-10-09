#![cfg_attr(not(test), allow(dead_code))]
use kiss::test_records::{TestRecord, load_records, records_dir};

use crate::test_runner::lang_iface::{
    EnsureRequest, ExecutionWitness, LanguageRuntime, Listing, WitnessStatus,
};

pub(crate) struct StoredRows {
    pub(crate) witness: ExecutionWitness,
}

pub(crate) fn stored_rows(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    listing: &Listing,
) -> Option<StoredRows> {
    #[cfg(test)]
    if let Some(seeded) = module.seeded_rows(request) {
        return Some(StoredRows { witness: seeded });
    }
    let language = module.language();
    let mut rows: Vec<TestRecord> = Vec::new();
    for row in load_records(&records_dir(&request.repo_root, language.label())) {
        if row.identity != listing.record_identity {
            continue;
        }
        if module.deps(request, &row).is_none() {
            continue;
        }
        rows.push(row);
    }
    if rows.is_empty() {
        return None;
    }
    rows.sort_by(|a, b| a.test_id.cmp(&b.test_id));
    let statuses: Vec<WitnessStatus> = rows
        .iter()
        .map(|row| WitnessStatus::from_test_status(row.status))
        .collect();
    let witness = ExecutionWitness {
        language,
        identity_digest: listing.identity.clone(),
        generation_id: rows_digest(&listing.record_identity, &rows),
        selectors: rows.iter().map(|row| row.test_id.clone()).collect(),
        durations_ns: rows
            .iter()
            .map(|row| u64::try_from(row.duration.as_nanos()).ok())
            .collect(),
        complete: statuses
            .iter()
            .all(|status| *status == WitnessStatus::Passed),
        raw_statuses: statuses.clone(),
        statuses,
    };
    Some(StoredRows { witness })
}

fn rows_digest(identity: &str, rows: &[TestRecord]) -> String {
    let mut text = identity.to_string();
    for row in rows {
        text.push_str(&format!(
            "\n{}\t{:?}\t{}",
            row.test_id,
            row.status,
            row.duration.as_nanos()
        ));
    }
    format!(
        "{:016x}",
        crate::analyze_cache::fnv1a64(0xcbf2_9ce4_8422_2325, text.as_bytes())
    )
}
