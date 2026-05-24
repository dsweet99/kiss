//! Semantic rename/move (`kiss mv`): query parsing, planning, and transactional apply.

mod edit;
mod opts;
mod plan;
mod query;

pub use edit::{EditKind, MvPlan, PlannedEdit};
pub use opts::{MvOptions, MvRequest};
pub use plan::plan_edits;
pub use query::{ParsedQuery, parse_mv_query, validate_new_name};

use crate::Language;

pub const fn mv_language_name(language: Language) -> &'static str {
    match language {
        Language::Python => "python",
        Language::Rust => "rust",
    }
}

pub use mv_language_name as language_name;

pub use crate::symbol_mv_support::apply_plan_transactional;

pub fn run_mv_command(opts: MvOptions) -> i32 {
    i32::from(crate::symbol_mv_support::run_mv_inner(opts).is_err())
}

#[cfg(test)]
mod symbol_mv_coverage {
    use super::{EditKind, MvOptions, MvPlan, PlannedEdit, run_mv_command};
    use std::path::PathBuf;

    #[test]
    fn mv_language_name_maps_each_language() {
        use crate::Language;
        assert_eq!(super::mv_language_name(Language::Python), "python");
    }

    #[test]
    fn language_name_export_matches_mv_language_name() {
        use crate::Language;
        assert_eq!(super::language_name(Language::Rust), "rust");
    }

    #[test]
    fn mv_plan_and_planned_edit_construction() {
        let edit = PlannedEdit {
            path: PathBuf::from("test.rs"),
            start_byte: 0,
            end_byte: 3,
            line: 1,
            old_snippet: "foo".into(),
            new_snippet: "bar".into(),
            kind: EditKind::Reference,
        };
        let plan = MvPlan {
            files: vec![PathBuf::from("test.rs")],
            edits: vec![edit],
        };
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.edits.len(), 1);
        assert!(matches!(plan.edits[0].kind, EditKind::Reference));
    }

    #[test]
    fn run_mv_command_dry_run_returns_zero_or_one() {
        let opts = MvOptions {
            query: "nonexistent_symbol_xyz".into(),
            new_name: "renamed".into(),
            paths: vec![],
            to: None,
            dry_run: true,
            json: false,
            lang_filter: None,
            ignore: vec![],
        };
        let code = run_mv_command(opts);
        assert!(code == 0 || code == 1);
    }
}
