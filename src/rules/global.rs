use super::{RuleSpec, ThresholdValue};

pub(super) const GLOBAL_RULE_SPECS: &[RuleSpec] = &[
    RuleSpec {
        metric: "min_similarity",
        op: ">=",
        threshold: ThresholdValue::F64(|_, g| g.min_similarity),
        description: "min_similarity is the minimum similarity required to report duplicate code (when duplication_enabled=true).",
    },
    RuleSpec {
        metric: "comment",
        op: "==",
        threshold: ThresholdValue::Usize(|_, _| 0),
        description: "comment counts non-doc comments. Enforced only when comment_removal_enabled=true. Python docstrings, Rust doc comments (///, //!, /**, /*!), and Rust clap CLI help comments are not counted.",
    },
    RuleSpec {
        metric: "doc",
        op: "==",
        threshold: ThresholdValue::Usize(|_, _| 0),
        description: "doc counts Python docstrings (including attribute docs) and Rust doc comments (///, //!, /**, /*!) plus `#[doc]` / `#![doc]` attributes. Allowed only under docs_allowed directory prefixes (relative to the repository root). Empty docs_allowed allows documentation in no directory. Default is []. Rust clap CLI help comments on Parser/Subcommand/Args/ValueEnum items and their fields/variants are exempt.",
    },
];
