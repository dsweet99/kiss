mod expand;
mod model;
mod model_python;
mod model_rust;
mod parse;
mod resolve;

use std::path::Path;

use kiss::Language;

pub(crate) use expand::{ExpandedTargetPlan, expand_target_operands};

pub(crate) fn rust_direct_test_selectors(path: &Path) -> Result<Vec<String>, String> {
    let model = model::load_source_model(path, Language::Rust)?;
    Ok(model
        .direct_tests
        .into_iter()
        .map(|test| test.selector)
        .filter(|selector| !selector.is_empty())
        .collect())
}

#[cfg(test)]
pub(crate) use parse::parse_test_target;
pub(crate) use resolve::{TargetSelectionQuery, resolve_target_operands};

pub(super) fn language_label(language: Language) -> &'static str {
    language.label()
}

#[cfg(test)]
#[path = "targets_b_test.rs"]
mod targets_b_test;
#[cfg(test)]
#[path = "targets_expand_test.rs"]
mod targets_expand_test;
#[cfg(test)]
#[path = "targets_test.rs"]
mod targets_test;
