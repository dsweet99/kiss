//! Accept-rule helpers for execution witnesses.

use std::collections::BTreeMap;
use std::time::Duration;

use kiss::GateConfig;
use rpytest_runner::TestStatus;

use crate::test_runner::runners::{
    SelectorCacheRecord, SelectorExecutionRecord, SelectorExecutionSummary,
};
use crate::test_runner::status_labels::apply_unit_test_time_limit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WitnessScope {
    Full,
    Subset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AcceptMode {
    All,
    Subset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WitnessStatus {
    Passed,
    Failed,
    TimedOut,
    Unresolved,
}

impl WitnessStatus {
    pub(crate) fn from_test_status(status: TestStatus) -> Self {
        match status {
            TestStatus::Passed => Self::Passed,
            TestStatus::Failed => Self::Failed,
            TestStatus::TimedOut => Self::TimedOut,
        }
    }

    pub(crate) fn to_test_status(self) -> Option<TestStatus> {
        match self {
            Self::Passed => Some(TestStatus::Passed),
            Self::Failed => Some(TestStatus::Failed),
            Self::TimedOut => Some(TestStatus::TimedOut),
            Self::Unresolved => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Unresolved => "unresolved",
        }
    }

    pub(crate) fn parse(raw: &str) -> Self {
        match raw {
            "passed" | "Passed" | "PASS" => Self::Passed,
            "failed" | "Failed" | "FAIL" => Self::Failed,
            "timed_out" | "TimedOut" | "TIMEOUT" => Self::TimedOut,
            _ => Self::Unresolved,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExecutionWitness {
    pub(crate) language: String,
    pub(crate) scope: WitnessScope,
    pub(crate) identity_digest: String,
    pub(crate) selectors: Vec<String>,
    pub(crate) statuses: Vec<WitnessStatus>,
    pub(crate) durations_ns: Vec<Option<u64>>,
    /// Aggregate covered lines (repo-relative paths). Empty when unknown.
    pub(crate) covered_lines: BTreeMap<String, Vec<u32>>,
    pub(crate) complete: bool,
    pub(crate) generation_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AcceptDecision {
    Accept,
    Miss(&'static str),
}

pub(crate) fn accept_witness(
    mode: AcceptMode,
    planned_selectors: &[String],
    current_identity_digest: &str,
    witness: &ExecutionWitness,
) -> AcceptDecision {
    if let Some(miss) = shape_or_identity_miss(witness, current_identity_digest) {
        return miss;
    }
    let mut planned = planned_selectors.to_vec();
    planned.sort();
    planned.dedup();
    if planned.is_empty() {
        return AcceptDecision::Miss("empty_plan");
    }
    match mode {
        AcceptMode::All => accept_all(&planned, witness),
        AcceptMode::Subset => accept_subset(&planned, witness),
    }
}

/// Selectors the ensure kernel must run after a Miss (or all planned on force / no witness).
pub(crate) fn miss_selectors_for_repair(
    mode: AcceptMode,
    planned_selectors: &[String],
    current_identity_digest: &str,
    witness: Option<&ExecutionWitness>,
    force: bool,
) -> Vec<String> {
    let mut planned = planned_selectors.to_vec();
    planned.sort();
    planned.dedup();
    if planned.is_empty() || force {
        return planned;
    }
    let Some(witness) = witness else {
        return planned;
    };
    match accept_witness(mode, &planned, current_identity_digest, witness) {
        AcceptDecision::Accept => Vec::new(),
        AcceptDecision::Miss(reason) => match reason {
            "incomplete" | "non_passed" | "missing_selector" => {
                non_passed_planned(&planned, witness)
            }
            _ => planned,
        },
    }
}

/// Prior-failure selectors must re-run even when the witness still says Passed
/// (failure skips witness publish, leaving stale Passed). Does not batch-force.
pub(crate) fn union_force_selectors_into_misses(
    planned: &[String],
    misses: &mut Vec<String>,
    force_selectors: &[String],
) {
    for sel in force_selectors {
        if planned.iter().any(|p| p == sel) && !misses.iter().any(|m| m == sel) {
            misses.push(sel.clone());
        }
    }
}

fn non_passed_planned(planned: &[String], witness: &ExecutionWitness) -> Vec<String> {
    let index = selector_index(&witness.selectors);
    planned
        .iter()
        .filter(|sel| match index.get(sel.as_str()) {
            Some(&i) => witness.statuses[i] != WitnessStatus::Passed,
            None => true,
        })
        .cloned()
        .collect()
}

fn shape_or_identity_miss(
    witness: &ExecutionWitness,
    current_identity_digest: &str,
) -> Option<AcceptDecision> {
    if witness.selectors.len() != witness.statuses.len()
        || witness.selectors.len() != witness.durations_ns.len()
    {
        return Some(AcceptDecision::Miss("witness_shape"));
    }
    if witness.identity_digest != current_identity_digest {
        return Some(AcceptDecision::Miss("identity"));
    }
    None
}

fn accept_all(planned: &[String], witness: &ExecutionWitness) -> AcceptDecision {
    if witness.scope != WitnessScope::Full {
        return AcceptDecision::Miss("scope_subset");
    }
    if !witness.complete {
        return AcceptDecision::Miss("incomplete");
    }
    if planned != witness.selectors.as_slice() {
        return AcceptDecision::Miss("selector_universe");
    }
    if witness.statuses.iter().any(|s| *s != WitnessStatus::Passed) {
        return AcceptDecision::Miss("non_passed");
    }
    AcceptDecision::Accept
}

fn accept_subset(planned: &[String], witness: &ExecutionWitness) -> AcceptDecision {
    let index = selector_index(&witness.selectors);
    for selector in planned {
        let Some(&i) = index.get(selector.as_str()) else {
            return AcceptDecision::Miss("missing_selector");
        };
        if witness.statuses[i] != WitnessStatus::Passed {
            return AcceptDecision::Miss("non_passed");
        }
    }
    AcceptDecision::Accept
}

fn selector_index(selectors: &[String]) -> BTreeMap<&str, usize> {
    selectors
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect()
}

/// Reclassify raw stored statuses under the current unit-test time-limit gate.
///
/// Ownership: this is the authoritative warm-path application of
/// `max_unit_test_seconds`. Live runners may also call `apply_unit_test_time_limit`
/// for immediate stdout/summary labeling; stored witness rows are reclassified
/// here so a later gate change still takes effect on accept without re-running.
///
/// Missing timings (`None`) are not collapsed to zero: under an active time gate,
/// a Passed row without a duration fails closed to Failed so repair re-runs it
/// (TimedOut would warm-skip and strand the selector without a measured time).
pub(crate) fn reclassify_statuses_with_gate(
    selectors: &[String],
    raw_statuses: &[WitnessStatus],
    durations_ns: &[Option<u64>],
    gate: &GateConfig,
) -> Vec<WitnessStatus> {
    selectors
        .iter()
        .zip(raw_statuses.iter())
        .zip(durations_ns.iter())
        .map(|((selector, raw), ns)| {
            let Some(base) = raw.to_test_status() else {
                return WitnessStatus::Unresolved;
            };
            let Some(ns) = *ns else {
                if gate.unit_test_time_gate_disabled() {
                    return WitnessStatus::from_test_status(base);
                }
                return match base {
                    TestStatus::Passed => WitnessStatus::Failed,
                    other => WitnessStatus::from_test_status(other),
                };
            };
            let effective = apply_unit_test_time_limit(
                base,
                selector,
                Duration::from_nanos(ns),
                gate,
            );
            WitnessStatus::from_test_status(effective)
        })
        .collect()
}

/// Emit today's cache-hit reporting contract without spawning runners.
pub(crate) fn summary_from_accepted_witness(
    planned_selectors: &[String],
    witness: &ExecutionWitness,
    report_id: impl Fn(&str) -> String,
) -> SelectorExecutionSummary {
    summary_from_witness_statuses(planned_selectors, witness, report_id, true)
}

/// Report planned selectors from a witness, including non-Passed terminals.
/// When `require_all_passed`, every planned selector must be Passed (Accept path).
pub(crate) fn summary_from_witness_statuses(
    planned_selectors: &[String],
    witness: &ExecutionWitness,
    report_id: impl Fn(&str) -> String,
    require_all_passed: bool,
) -> SelectorExecutionSummary {
    let index = selector_index(&witness.selectors);
    let mut planned = planned_selectors.to_vec();
    planned.sort();
    planned.dedup();
    let records = planned_witness_records(&planned, witness, &index, &report_id, require_all_passed);
    emit_cached_witness_lines(&records);
    let mut summary = SelectorExecutionSummary::default();
    for (report, status, duration) in records {
        summary.record(SelectorExecutionRecord {
            selector: report,
            status,
            raw_status: None,
            cache_record: SelectorCacheRecord::Hit,
            exit_code: Some(exit_code_for_test_status(status)),
            duration,
        });
    }
    summary
}

fn planned_witness_records(
    planned: &[String],
    witness: &ExecutionWitness,
    index: &std::collections::BTreeMap<&str, usize>,
    report_id: &impl Fn(&str) -> String,
    require_all_passed: bool,
) -> Vec<(String, TestStatus, Duration)> {
    let mut records = Vec::with_capacity(planned.len());
    for selector in planned {
        let i = index[selector.as_str()];
        let status = witness.statuses[i]
            .to_test_status()
            .unwrap_or(TestStatus::Failed);
        if require_all_passed {
            assert_eq!(status, TestStatus::Passed);
        }
        records.push((
            report_id(selector),
            status,
            Duration::from_nanos(witness.durations_ns[i].unwrap_or(0)),
        ));
    }
    records
}

fn emit_cached_witness_lines(records: &[(String, TestStatus, Duration)]) {
    if records.len() <= 64 {
        for (report, status, duration) in records {
            crate::test_runner::status_labels::print_classified_status_line(
                *status,
                report,
                *duration,
                Some("cached"),
                false,
            );
        }
        return;
    }
    let (passed, failed, timed_out) = count_test_statuses(records);
    print_cached_total("PASS", passed);
    print_cached_total("FAIL", failed);
    print_cached_total("TIMEOUT", timed_out);
}

fn count_test_statuses(records: &[(String, TestStatus, Duration)]) -> (usize, usize, usize) {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut timed_out = 0usize;
    for (_, status, _) in records {
        match status {
            TestStatus::Passed => passed += 1,
            TestStatus::Failed => failed += 1,
            TestStatus::TimedOut => timed_out += 1,
        }
    }
    (passed, failed, timed_out)
}

fn print_cached_total(label: &str, count: usize) {
    if count > 0 {
        println!("{label} (cached): {count} selectors");
    }
}

fn exit_code_for_test_status(status: TestStatus) -> i32 {
    match status {
        TestStatus::Passed => 0,
        TestStatus::Failed => 1,
        TestStatus::TimedOut => 124,
    }
}

pub(crate) fn all_misses_warm_skippable(witness: &ExecutionWitness, misses: &[String]) -> bool {
    if misses.is_empty() {
        return false;
    }
    let index = selector_index(&witness.selectors);



    misses.iter().all(|sel| match index.get(sel.as_str()) {
        Some(&i) => matches!(
            witness.statuses[i],
            WitnessStatus::TimedOut | WitnessStatus::Unresolved
        ),
        None => false,
    })
}
