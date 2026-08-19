
mod expand;
mod model;
mod model_python;
mod model_rust;
mod parse;
mod resolve;

use kiss::Language;

pub(crate) use expand::{ExpandedTargetPlan, expand_target_operands};
#[cfg(test)]
pub(crate) use parse::parse_test_target;
pub(crate) use resolve::{TargetSelectionQuery, resolve_target_operands};

pub(super) fn language_label(language: Language) -> &'static str {
    language.label()
}

#[cfg(test)]
#[path = "targets_test.rs"]
mod targets_test;
#[cfg(test)]
#[path = "targets_b_test.rs"]
mod targets_b_test;
#[cfg(test)]
#[path = "targets_expand_test.rs"]
mod targets_expand_test;
