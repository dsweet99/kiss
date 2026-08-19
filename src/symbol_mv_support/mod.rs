
mod ast_models;
mod ast_plan;
mod ast_python;
mod ast_python_walk;
mod ast_rust;
#[path = "ast_rust_macros.rs"]
mod ast_rust_macros;
#[path = "ast_rust_span.rs"]
mod ast_rust_span;
#[path = "ast_rust_visitors.rs"]
mod ast_rust_visitors;
mod basics;
mod definition;
mod edits;
mod identifiers;
mod lex;
mod lex_fstring;
mod lex_rust;
mod reference;
mod reference_inference;
mod run_mv;
mod transaction;

pub use basics::{
    detect_language, gather_candidate_files, is_valid_identifier, parse_symbol_shape,
};
pub use definition::DefinitionSpan;
pub use definition::find_definition_span;
pub use edits::{
    MoveEditsParams, ReferenceRenameParams, SourceRenameParams, build_move_edits,
    collect_reference_edits, collect_source_rename_edits,
};
pub use run_mv::run_mv_inner;
pub use transaction::apply_plan_transactional;

pub(crate) use ast_plan::PlanInvocationGuard;

#[cfg(test)]
#[path = "symbol_mv_support_test.rs"]
mod symbol_mv_support_test;
