//! Helpers for `kiss mv`: identifier spans, reference filtering, transactional apply.

mod basics;
mod definition;
mod edits;
mod identifiers;
mod lex;
mod reference;
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

#[cfg(test)]
#[path = "symbol_mv_support_test.rs"]
mod symbol_mv_support_test;
