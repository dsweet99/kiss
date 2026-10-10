use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;

use super::config::SelectorIndex;
use super::records::{Outcome, store};
use super::status_line::FinishedTest;
use crate::test_runner::runners::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary,
};

fn severity(status: TestStatus) -> u8 {
    match status {
        TestStatus::Passed => 0,
        TestStatus::Failed => 1,
        TestStatus::TimedOut => 2,
    }
}

pub(super) struct Plan {
    pub(super) identity: String,
    pub(super) inputs: String,
    pub(super) report_ids: BTreeMap<String, String>,
    pub(super) timeout_millis: BTreeMap<String, u64>,
    pub(super) index: SelectorIndex,
    pub(super) cache_policy: kiss::test_cache_policy::TestCachePolicy,
}

impl Plan {
    fn report_id<'a>(&'a self, selector: &'a str) -> &'a str {
        self.report_ids
            .get(selector)
            .map_or(selector, String::as_str)
    }
}

pub(super) struct Results<'a> {
    repo_root: &'a Path,
    gate: &'a kiss::GateConfig,
    plan: Plan,
    seen: BTreeMap<String, (TestStatus, Duration)>,
    requested: usize,
}

impl<'a> Results<'a> {
    pub(super) fn new(
        repo_root: &'a Path,
        gate: &'a kiss::GateConfig,
        plan: Plan,
        requested: usize,
    ) -> Self {
        Self {
            repo_root,
            gate,
            plan,
            seen: BTreeMap::new(),
            requested,
        }
    }

    pub(super) fn refresh_inputs(&mut self) -> Result<(), String> {
        crate::test_runner::workspace_selector_cache::forget_inventory(self.repo_root);
        self.plan.inputs = super::records::rust_inputs_digest(self.repo_root)?;
        Ok(())
    }

    pub(super) fn finished(&mut self, test: &FinishedTest) -> Result<(), String> {
        let selectors: Vec<String> = self
            .plan
            .index
            .selectors_for(test)
            .into_iter()
            .map(str::to_string)
            .collect();
        for selector in selectors {
            self.merge(&selector, test.status, test.duration)?;
        }
        Ok(())
    }

    pub(super) fn skipped(&mut self, binary_id: &str, test_name: &str) -> Result<(), String> {
        self.finished(&FinishedTest {
            status: TestStatus::Passed,
            duration: Duration::ZERO,
            binary_id: binary_id.to_string(),
            test_name: test_name.to_string(),
        })
    }

    fn merge(
        &mut self,
        selector: &str,
        status: TestStatus,
        duration: Duration,
    ) -> Result<(), String> {
        let previous = self.seen.get(selector).copied();
        let merged = match previous {
            Some((old, old_duration)) => (
                if severity(status) > severity(old) {
                    status
                } else {
                    old
                },
                duration.max(old_duration),
            ),
            None => (status, duration),
        };
        if previous == Some(merged) {
            return Ok(());
        }
        self.seen.insert(selector.to_string(), merged);
        store(
            self.repo_root,
            &self.plan.identity,
            &self.plan.inputs,
            &Outcome {
                test_id: selector,
                status: merged.0,
                duration: merged.1,
                timeout_ms: self.plan.timeout_millis.get(selector).copied(),
                declared_inputs: super::records::declared_inputs_digest(
                    self.repo_root,
                    &self.plan.cache_policy,
                    selector,
                ),
            },
        )?;
        if previous.is_none_or(|(old, _)| old != merged.0) {
            let report_id = self.plan.report_id(selector);
            let effective = self.effective(merged.0, report_id, merged.1);
            crate::test_runner::status_labels::print_classified_status_line(
                effective, report_id, merged.1, None, true,
            );
        }
        crate::test_runner::tests_remaining::set_language_remaining(
            kiss::Language::Rust,
            self.requested.saturating_sub(self.seen.len()),
        );
        Ok(())
    }

    fn effective(&self, status: TestStatus, report_id: &str, duration: Duration) -> TestStatus {
        crate::test_runner::status_labels::apply_unit_test_time_limit(
            status, report_id, duration, self.gate,
        )
    }

    pub(super) fn missing<'s>(&self, selectors: &'s [String]) -> Vec<&'s str> {
        selectors
            .iter()
            .filter(|selector| !self.seen.contains_key(*selector))
            .map(String::as_str)
            .collect()
    }

    pub(super) fn finish(
        self,
        on_result: &mut dyn FnMut(SelectorExecutionRecord),
    ) -> SelectorExecutionSummary {
        let mut summary = SelectorExecutionSummary::default();
        for (selector, (status, duration)) in &self.seen {
            let report_id = self.plan.report_id(selector);
            on_result(SelectorExecutionRecord {
                selector: report_id.to_string(),
                status: self.effective(*status, report_id, *duration),
                raw_status: Some(*status),
                cache_record: SelectorCacheRecord::MissStored,
                exit_code: Some(i32::from(*status != TestStatus::Passed)),
                duration: *duration,
            });
            summary.raw_statuses.insert(selector.clone(), *status);
            summary.selector_durations_ns.insert(
                selector.clone(),
                u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX),
            );
        }
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished(binary_id: &str, status: TestStatus, millis: u64) -> FinishedTest {
        FinishedTest {
            status,
            duration: Duration::from_millis(millis),
            binary_id: binary_id.into(),
            test_name: "t::case".into(),
        }
    }

    #[test]
    fn bare_selector_takes_worst_status_and_longest_duration_and_is_stored() {
        let tmp = tempfile::tempdir().unwrap();
        let selectors = vec!["t::case".to_string(), "t::never".to_string()];
        let plan = Plan {
            identity: "id".into(),
            inputs: "inputs".into(),
            report_ids: BTreeMap::from([("t::case".into(), "src/lib.rs::t::case".into())]),
            timeout_millis: BTreeMap::new(),
            index: SelectorIndex::new(&selectors, &Default::default()),
            cache_policy: Default::default(),
        };
        let gate = kiss::GateConfig::default();
        let mut results = Results::new(tmp.path(), &gate, plan, selectors.len());
        results
            .finished(&finished("p", TestStatus::Passed, 9))
            .unwrap();
        results
            .finished(&finished("p::bin/x", TestStatus::Failed, 2))
            .unwrap();
        results
            .finished(&finished("p::it", TestStatus::Passed, 4))
            .unwrap();
        assert_eq!(results.missing(&selectors), ["t::never"]);

        let stored = super::super::records::records_under(tmp.path(), "id");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].status, TestStatus::Failed);
        assert_eq!(stored[0].duration, Duration::from_millis(9));

        let mut records = Vec::new();
        let summary = results.finish(&mut |record| records.push(record));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].selector, "src/lib.rs::t::case");
        assert_eq!(records[0].status, TestStatus::Failed);
        assert_eq!(summary.raw_statuses["t::case"], TestStatus::Failed);
        assert_eq!(summary.selector_durations_ns["t::case"], 9_000_000);
    }

    #[test]
    fn skipped_test_counts_as_a_zero_duration_pass() {
        let tmp = tempfile::tempdir().unwrap();
        let selectors = vec!["t::case".to_string()];
        let plan = Plan {
            identity: "id".into(),
            inputs: "inputs".into(),
            report_ids: BTreeMap::new(),
            timeout_millis: BTreeMap::new(),
            index: SelectorIndex::new(&selectors, &Default::default()),
            cache_policy: Default::default(),
        };
        let gate = kiss::GateConfig::default();
        let mut results = Results::new(tmp.path(), &gate, plan, 1);
        results.skipped("p", "t::case").unwrap();
        assert!(results.missing(&selectors).is_empty());
        let summary = results.finish(&mut |_| {});
        assert_eq!(summary.raw_statuses["t::case"], TestStatus::Passed);
    }
}
