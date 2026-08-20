use super::{RuleSpec, ThresholdValue};

pub(super) const TEST_RULE_SPECS: &[RuleSpec] = &[
    RuleSpec {
        metric: "test_coverage_threshold",
        op: ">=",
        threshold: ThresholdValue::Usize(|_, g| g.test_coverage_threshold),
        description: "test_coverage_threshold is the minimum percent of syntactically coverable source lines covered by cached runtime coverage (rslip for Python, llvm-cov for Rust). Enforcement is per file or codebase-wide according to `test_coverage_scope` (default `codebase`). `kiss test` uses a current cache. Missing or stale Python coverage is refreshed before enforcement. When Rust coverage is stale, kiss reuses selector coverage only when the compiled test executable digest is unchanged, reruns invalidated selectors, and falls back to a full Rust test-population refresh whenever reuse cannot be proven safe.",
    },
    RuleSpec {
        metric: "max_unit_test_seconds",
        op: "<",
        threshold: ThresholdValue::F64(|_, g| g.catch_all_unit_test_seconds()),
        description: "max_unit_test_seconds is an ordered path-pattern → seconds table (must end with \"*\"). First match wins. Enforced by `kiss test` and used by `kiss test` for TIMEOUT labeling. Catch-all 0 bans unmatched paths. Default \"*\" = 2.0.",
    },
    RuleSpec {
        metric: "max_num_tests",
        op: "<=",
        threshold: ThresholdValue::Usize(|_, g| g.max_num_tests),
        description: "max_num_tests is the maximum number of unit tests in the current population (Python + Rust). Enforced by `kiss test` alongside coverage. `0` means any test fails. Default is 999999. Config key lives under `[test]`.",
    },
];
