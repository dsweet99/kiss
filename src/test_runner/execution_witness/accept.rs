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
    pub(crate) durations_ns: Vec<u64>,
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
pub(crate) fn reclassify_statuses_with_gate(
    selectors: &[String],
    raw_statuses: &[WitnessStatus],
    durations_ns: &[u64],
    gate: &GateConfig,
) -> Vec<WitnessStatus> {
    selectors
        .iter()
        .zip(raw_statuses.iter())
        .zip(durations_ns.iter())
        .map(|((selector, raw), &ns)| {
            let Some(base) = raw.to_test_status() else {
                return WitnessStatus::Unresolved;
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
    let index = selector_index(&witness.selectors);
    let mut summary = SelectorExecutionSummary::default();
    let mut planned = planned_selectors.to_vec();
    planned.sort();
    planned.dedup();
    for selector in planned {
        let i = index[selector.as_str()];
        let report = report_id(&selector);
        println!("PASS (cached): {report}");
        summary.record(SelectorExecutionRecord {
            selector: report,
            status: TestStatus::Passed,
            cache_record: SelectorCacheRecord::Hit,
            exit_code: Some(0),
            duration: Duration::from_nanos(witness.durations_ns[i]),
        });
    }
    summary
}
