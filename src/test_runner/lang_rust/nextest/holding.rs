use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kiss::rpytest_runner::TestStatus;
use kiss::test_records::{Selection, TestRecord, must_run};

use super::records::{
    cache_policy, current_deps, declared_inputs_digest, record_identity, records_under,
    rust_inputs_digest,
};

/// The time limit, in milliseconds, nextest enforces on the test reported as
/// `report_id`; `None` when the time gate is off.
pub(crate) fn timeout_millis(gate: &kiss::GateConfig, report_id: &str) -> Option<u64> {
    if gate.unit_test_time_gate_disabled() {
        return None;
    }
    let secs = kiss::limit_for_selector(&gate.max_unit_test_seconds, report_id);
    (secs.is_finite() && secs > 0.0).then(|| (secs * 1000.0).round().max(1.0) as u64)
}

/// Computes the current value of each dependency a Rust record keeps.
pub(crate) struct CurrentDeps {
    repo_root: PathBuf,
    gate: kiss::GateConfig,
    inputs: String,
    cache_policy: kiss::test_cache_policy::TestCachePolicy,
    report_ids: OnceCell<BTreeMap<String, String>>,
}

impl CurrentDeps {
    pub(crate) fn new(repo_root: &Path, gate: &kiss::GateConfig) -> Result<Self, String> {
        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            gate: gate.clone(),
            inputs: rust_inputs_digest(repo_root)?,
            cache_policy: cache_policy(repo_root),
            report_ids: OnceCell::new(),
        })
    }

    pub(crate) fn of(&self, row: &TestRecord) -> BTreeMap<String, String> {
        let declared = declared_inputs_digest(&self.repo_root, &self.cache_policy, &row.test_id);
        current_deps(&self.inputs, declared, row, || {
            let report_ids = self.report_ids.get_or_init(|| {
                crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(
                    &self.repo_root,
                    &[],
                )
                .unwrap_or_default()
            });
            let report_id = report_ids.get(&row.test_id).unwrap_or(&row.test_id);
            timeout_millis(&self.gate, report_id)
        })
    }
}

/// The record identity for a run with test arguments `extras`, and the stored records
/// made under it whose dependencies are unchanged.
pub(crate) fn holding_records(
    repo_root: &Path,
    extras: &[String],
    gate: &kiss::GateConfig,
) -> Result<(String, Vec<TestRecord>), String> {
    let identity = record_identity(repo_root, extras)?;
    let rows = records_under(repo_root, &identity);
    if rows.is_empty() {
        return Ok((identity, rows));
    }
    let deps = CurrentDeps::new(repo_root, gate)?;
    let holding = rows
        .into_iter()
        .filter(|row| {
            let current = deps.of(row);
            let selection = Selection {
                identity: &identity,
                current_deps: Some(&current),
                retry_bad: false,
                needs_duration: false,
            };
            must_run(Some(row.view()), &selection).is_none()
        })
        .collect();
    Ok((identity, holding))
}

/// Tests whose stored record under the current identity is FAIL or TIMEOUT, whether
/// or not that record still holds.
pub(crate) fn bad_record_ids(repo_root: &Path, extras: &[String]) -> Vec<String> {
    let Ok(identity) = record_identity(repo_root, extras) else {
        return Vec::new();
    };
    records_under(repo_root, &identity)
        .into_iter()
        .filter(|row| row.status != TestStatus::Passed)
        .map(|row| row.test_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeouts_follow_the_gate_and_never_round_to_zero() {
        let mut gate = kiss::GateConfig {
            max_unit_test_seconds: vec![("tests/".into(), 2.5), ("*".into(), 0.0001)],
            ..Default::default()
        };
        assert_eq!(timeout_millis(&gate, "src/a.rs::t::x"), Some(1));
        assert_eq!(timeout_millis(&gate, "tests/it.rs::x"), Some(2500));
        gate.max_unit_test_seconds.clear();
        assert_eq!(timeout_millis(&gate, "src/a.rs::t::x"), None);
    }

    #[test]
    fn bad_records_stay_retryable_after_their_inputs_change() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        super::super::store_records(
            root,
            &[
                ("t::ok", TestStatus::Passed),
                ("t::bad", TestStatus::Failed),
                ("t::slow", TestStatus::TimedOut),
            ],
        );
        std::fs::write(root.join("src/lib.rs"), "pub fn b() {}\n").unwrap();
        let mut bad = bad_record_ids(root, &[]);
        bad.sort();
        assert_eq!(bad, ["t::bad", "t::slow"]);
    }
}
