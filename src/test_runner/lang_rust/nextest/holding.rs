use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kiss::test_records::{Selection, TestRecord, must_run};

use super::records::{
    cache_policy, current_deps, declared_inputs_digest, record_identity, records_under,
    rust_inputs_digest,
};

pub(crate) fn timeout_millis(gate: &kiss::GateConfig, report_id: &str) -> Option<u64> {
    if gate.unit_test_time_gate_disabled() {
        return None;
    }
    let secs = kiss::limit_for_selector(&gate.max_unit_test_seconds, report_id);
    (secs.is_finite() && secs > 0.0).then(|| (secs * 1000.0).round().max(1.0) as u64)
}

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
                needs_duration: false,
            };
            must_run(Some(row.view()), &selection).is_none()
        })
        .collect();
    Ok((identity, holding))
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
}
