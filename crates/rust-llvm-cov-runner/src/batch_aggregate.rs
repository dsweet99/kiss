use std::collections::BTreeMap;
use std::time::Duration;

use rpytest_runner::TestStatus;

use crate::batch_events::selector_matches_test;
use crate::{RustCovCacheStatus, RustLineCoverage, RustLlvmCovOutcome};

#[derive(Clone, Debug, PartialEq)]
pub struct InstanceResult {
    pub full_name: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub duration: Duration,
    pub stdout: Option<Vec<u8>>,
    pub stderr: Option<Vec<u8>>,
    pub coverage: RustLineCoverage,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AggregationCounters {
    pub unmatched_selectors: usize,
    pub test_instances: usize,
}

pub fn aggregate_logical_selectors(
    selectors: &[String],
    exact: bool,
    instances: &[InstanceResult],
) -> (Vec<RustLlvmCovOutcome>, AggregationCounters) {
    let mut counters = AggregationCounters {
        test_instances: instances.len(),
        ..Default::default()
    };
    let mut outcomes = Vec::with_capacity(selectors.len());
    for selector in selectors {
        let matched: Vec<_> = instances
            .iter()
            .filter(|instance| selector_matches_test(&instance.full_name, selector, exact))
            .collect();
        if matched.is_empty() {
            counters.unmatched_selectors += 1;
            outcomes.push(successful_empty_outcome(selector));
            continue;
        }
        outcomes.push(aggregate_one_selector(selector, &matched));
    }
    (outcomes, counters)
}

fn successful_empty_outcome(selector: &str) -> RustLlvmCovOutcome {
    RustLlvmCovOutcome {
        selector: selector.to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::ZERO,
        coverage: RustLineCoverage {
            files: BTreeMap::new(),
        },
        cache_status: RustCovCacheStatus::MissStored,
        stdout: None,
        stderr: None,
    }
}

fn aggregate_one_selector(selector: &str, matched: &[&InstanceResult]) -> RustLlvmCovOutcome {
    let mut ordered: Vec<&InstanceResult> = matched.to_vec();
    ordered.sort_by(|left, right| left.full_name.cmp(&right.full_name));
    let failed = ordered.iter().any(|instance| !instance.passed);
    let duration: Duration = ordered.iter().map(|instance| instance.duration).sum();
    let mut stdout_parts = Vec::new();
    let mut stderr_parts = Vec::new();
    for instance in &ordered {
        if let Some(stdout) = &instance.stdout
            && !stdout.is_empty()
        {
            stdout_parts.push(stdout.clone());
        }
        if let Some(stderr) = &instance.stderr
            && !stderr.is_empty()
        {
            stderr_parts.push(stderr.clone());
        }
    }
    if failed {
        return RustLlvmCovOutcome {
            selector: selector.to_string(),
            status: TestStatus::Failed,
            exit_code: Some(1),
            duration,
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
            cache_status: RustCovCacheStatus::MissStored,
            stdout: concat_parts(stdout_parts),
            stderr: concat_parts(stderr_parts),
        };
    }
    RustLlvmCovOutcome {
        selector: selector.to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration,
        coverage: union_coverage(&ordered),
        cache_status: RustCovCacheStatus::MissStored,
        stdout: concat_parts(stdout_parts),
        stderr: concat_parts(stderr_parts),
    }
}

fn union_coverage(matched: &[&InstanceResult]) -> RustLineCoverage {
    let mut files = BTreeMap::new();
    for instance in matched {
        for (path, lines) in &instance.coverage.files {
            files
                .entry(path.clone())
                .or_insert_with(std::collections::BTreeSet::new)
                .extend(lines.iter().copied());
        }
    }
    RustLineCoverage { files }
}

fn concat_parts(parts: Vec<Vec<u8>>) -> Option<Vec<u8>> {
    if parts.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            out.push(b'\n');
        }
        out.extend_from_slice(part);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn instance(full_name: &str, passed: bool, line: u32) -> InstanceResult {
        InstanceResult {
            full_name: full_name.to_string(),
            passed,
            exit_code: Some(if passed { 0 } else { 1 }),
            duration: Duration::from_millis(2),
            stdout: None,
            stderr: None,
            coverage: RustLineCoverage {
                files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([line]))]),
            },
        }
    }

    #[test]
    fn two_selectors_matching_one_instance_union_coverage_once() {
        let instances = vec![instance("pkg::bin$alpha", true, 1)];
        let (outcomes, counters) =
            aggregate_logical_selectors(&["alpha".to_string(), "a".to_string()], false, &instances);
        assert_eq!(outcomes.len(), 2);
        assert!(
            outcomes
                .iter()
                .all(|outcome| outcome.status == TestStatus::Passed)
        );
        assert_eq!(
            outcomes[0].coverage.files["src/lib.rs"],
            BTreeSet::from([1])
        );
        assert_eq!(counters.test_instances, 1);
        assert_eq!(counters.unmatched_selectors, 0);
    }

    #[test]
    fn failed_instance_makes_selector_failed_with_empty_coverage() {
        let instances = vec![instance("pkg::bin$alpha", false, 1)];
        let (outcomes, _) = aggregate_logical_selectors(&["alpha".to_string()], false, &instances);
        assert_eq!(outcomes[0].status, TestStatus::Failed);
        assert_eq!(outcomes[0].exit_code, Some(1));
        assert!(outcomes[0].coverage.files.is_empty());
    }

    #[test]
    fn unmatched_selector_is_successful_empty() {
        let instances = vec![instance("pkg::bin$alpha", true, 1)];
        let (outcomes, counters) =
            aggregate_logical_selectors(&["missing".to_string()], false, &instances);
        assert_eq!(outcomes[0].status, TestStatus::Passed);
        assert_eq!(outcomes[0].exit_code, Some(0));
        assert_eq!(counters.unmatched_selectors, 1);
    }

    #[test]
    fn passed_selector_unions_multiple_instances_in_stable_binary_test_name_order() {
        let mut first = instance("pkg::bin_b$alpha", true, 1);
        first.stdout = Some(b"out-a".to_vec());
        let mut second = instance("pkg::bin_a$beta", true, 2);
        second.stdout = Some(b"out-b".to_vec());
        second.stderr = Some(b"err-b".to_vec());
        let instances = vec![first, second];
        let (outcomes, counters) =
            aggregate_logical_selectors(&["pkg".to_string()], false, &instances);
        assert_eq!(outcomes[0].status, TestStatus::Passed);
        assert_eq!(
            outcomes[0].coverage.files["src/lib.rs"],
            BTreeSet::from([1, 2])
        );
        assert_eq!(
            outcomes[0].stdout.as_deref(),
            Some(b"out-b\nout-a".as_ref())
        );
        assert_eq!(outcomes[0].stderr.as_deref(), Some(b"err-b".as_ref()));
        assert_eq!(counters.test_instances, 2);
    }

    #[test]
    fn aggregation_counter_and_instance_result_types_are_constructible() {
        let counter = AggregationCounters {
            unmatched_selectors: 1,
            test_instances: 2,
        };
        let instance = InstanceResult {
            full_name: "pkg::bin$alpha".to_string(),
            passed: true,
            exit_code: Some(0),
            duration: Duration::from_millis(3),
            stdout: None,
            stderr: None,
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
        };
        assert_eq!(counter.unmatched_selectors, 1);
        assert_eq!(instance.full_name, "pkg::bin$alpha");
    }
}
