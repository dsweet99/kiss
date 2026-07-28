//! Explicit `kiss test` PATH / PATH::symbol target parsing and resolution.

mod model;
mod model_python;
mod model_rust;
mod parse;
mod resolve;

use kiss::Language;

#[cfg(test)]
pub(crate) use parse::parse_test_target;
pub(crate) use resolve::resolve_target_operands;

pub(super) fn language_label(language: Language) -> &'static str {
    match language {
        Language::Python => "python",
        Language::Rust => "rust",
    }
}

#[cfg(test)]
#[path = "targets_test.rs"]
mod targets_test;
#[cfg(test)]
#[path = "targets_b_test.rs"]
mod targets_b_test;
