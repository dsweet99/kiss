use std::collections::{BTreeMap, BTreeSet};

use crate::{RustLineCoverage, RustLlvmCovOutcome};

#[cfg(test)]
pub(crate) fn attach_binary_line_maps_to_completed_outcomes(
    completed: &mut [RustLlvmCovOutcome],
    selector_binary_ids: &BTreeMap<String, Vec<String>>,
    binary_line_maps: &BTreeMap<String, RustLineCoverage>,
) {
    for outcome in completed {
        let Some(binary_ids) = selector_binary_ids.get(&outcome.selector) else {
            continue;
        };
        let mut files: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        for binary_id in binary_ids {
            let Some(coverage) = binary_line_maps.get(binary_id) else {
                continue;
            };
            for (path, lines) in &coverage.files {
                files
                    .entry(path.clone())
                    .or_default()
                    .extend(lines.iter().copied());
            }
        }
        outcome.coverage = RustLineCoverage { files };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RustCovCacheStatus;
    use rpytest_runner::TestStatus;
    use std::time::Duration;

    fn outcome(selector: &str) -> RustLlvmCovOutcome {
        RustLlvmCovOutcome {
            selector: selector.to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::ZERO,
            coverage: RustLineCoverage {
                files: BTreeMap::from([("old.rs".to_string(), BTreeSet::from([99]))]),
            },
            test_binary_ids: Vec::new(),
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        }
    }

    #[test]
    fn missing_selector_binary_ids_preserves_existing_coverage() {
        let mut completed = vec![outcome("alpha")];
        attach_binary_line_maps_to_completed_outcomes(
            &mut completed,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(completed[0].coverage.files["old.rs"], BTreeSet::from([99]));
    }

    #[test]
    fn missing_binary_line_map_clears_stale_coverage() {
        let mut completed = vec![outcome("alpha")];
        attach_binary_line_maps_to_completed_outcomes(
            &mut completed,
            &BTreeMap::from([("alpha".to_string(), vec!["bin-a".to_string()])]),
            &BTreeMap::new(),
        );
        assert!(completed[0].coverage.files.is_empty());
    }

    #[test]
    fn selector_coverage_unions_all_mapped_binaries() {
        let mut completed = vec![outcome("alpha")];
        let selector_binary_ids = BTreeMap::from([(
            "alpha".to_string(),
            vec!["bin-a".to_string(), "bin-b".to_string()],
        )]);
        let binary_line_maps = BTreeMap::from([
            (
                "bin-a".to_string(),
                RustLineCoverage {
                    files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
                },
            ),
            (
                "bin-b".to_string(),
                RustLineCoverage {
                    files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([2]))]),
                },
            ),
        ]);
        attach_binary_line_maps_to_completed_outcomes(
            &mut completed,
            &selector_binary_ids,
            &binary_line_maps,
        );
        assert_eq!(
            completed[0].coverage.files["src/lib.rs"],
            BTreeSet::from([1, 2])
        );
    }
}
