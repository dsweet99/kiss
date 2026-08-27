use super::{RuleSpec, ThresholdOp, ThresholdValue};

pub(super) const GLOBAL_RULE_SPECS: &[RuleSpec] = &[
    RuleSpec {
        metric: "min_similarity",
        op: ThresholdOp::AtLeast,
        threshold: ThresholdValue::F64(|_, g| g.min_similarity),
        description: "min_similarity is the minimum similarity required to report duplicate code (when duplication_enabled=true).",
    },
    RuleSpec {
        metric: "comment",
        op: ThresholdOp::Equal,
        threshold: ThresholdValue::Usize(|_, _| 0),
        description: "comment counts non-doc comments. Enforced only when comment_removal_enabled=true. Python docstrings, Rust doc comments (///, //!, /**, /*!), and Rust clap CLI help comments are not counted.",
    },
    RuleSpec {
        metric: "doc",
        op: ThresholdOp::Equal,
        threshold: ThresholdValue::Usize(|_, _| 0),
        description: "doc counts Python docstrings (including attribute docs) and Rust doc comments (///, //!, /**, /*!) plus `#[doc]` / `#![doc]` attributes. Allowed only under docs_allowed directory prefixes (relative to the repository root). Empty docs_allowed allows documentation in no directory. Default is []. Rust clap CLI help comments on Parser/Subcommand/Args/ValueEnum items and their fields/variants are exempt.",
    },
    RuleSpec {
        metric: "orphan_module",
        op: ThresholdOp::Equal,
        threshold: ThresholdValue::Usize(|_, _| 0),
        description: "orphan_module flags an isolated production module (GraphIsolation::IsolatedModule: production fan-in and fan-out both 0) with no static test-only import edges, that is not a recognized entry (name, AST, or manifest/Cargo), and whose path is not under orphan_allowed. Enforced only when orphan_module_enabled=true.",
    },
    RuleSpec {
        metric: "orphan",
        op: ThresholdOp::Equal,
        threshold: ThresholdValue::Usize(|_, _| 0),
        description: "orphan flags a production code unit (module, function, method, or class) that nothing in this repository uses: GraphIsolation::UnreferencedModule (no named import/use) and no runtime coverable line ran. Requires a coverage snapshot; a missing snapshot is not evaluated. Enforced by kiss test when orphan_unit_enabled=true (default false). Entries, tests, orphan_allowed paths, and empty __init__.py module units are not candidates.",
    },
];
