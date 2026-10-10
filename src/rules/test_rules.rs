use super::{RuleSpec, ThresholdOp, ThresholdValue};

pub(super) const TEST_RULE_SPECS: &[RuleSpec] = &[
    RuleSpec {
        metric: "max_unit_test_seconds",
        op: ThresholdOp::StrictLess,
        threshold: ThresholdValue::F64(|_, g| g.catch_all_unit_test_seconds()),
        description: "max_unit_test_seconds is an ordered path-pattern → seconds table (must end with \"*\"). First match wins. Enforced by `kiss test` and used by `kiss test` for TIMEOUT labeling. Catch-all 0 bans unmatched paths. Default \"*\" = 2.0.",
    },
    RuleSpec {
        metric: "orphan",
        op: ThresholdOp::Equal,
        threshold: ThresholdValue::Usize(|_, _| 0),
        description: "orphan flags a production code unit (module, function, method, or class) that flood-fill never reaches from tests, mains, or units that the tests executed. Enforced by kiss test after tests pass, when orphan_detection=true (default false). kiss check does not run orphan detection. Entries, tests, trait-impl methods, orphan_allowed paths, and __init__.py / mod.rs module units are not candidates.",
    },
];
