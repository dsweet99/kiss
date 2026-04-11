//! Semantic rename/move (`kiss mv`): query parsing, planning, and transactional apply.

mod edit;
mod opts;
mod plan;
mod query;

pub use edit::{EditKind, MvPlan, PlannedEdit};
pub use opts::{MvOptions, MvRequest};
pub use plan::plan_edits;
pub use query::{parse_mv_query, validate_new_name, ParsedQuery};

use crate::Language;

pub const fn language_name(language: Language) -> &'static str {
    match language {
        Language::Python => "python",
        Language::Rust => "rust",
    }
}

pub fn apply_plan_transactional(plan: &MvPlan) -> Result<(), String> {
    crate::symbol_mv_support::apply_plan_transactional(plan)
}

pub fn run_mv_command(opts: MvOptions) -> i32 {
    i32::from(crate::symbol_mv_support::run_mv_inner(opts).is_err())
}

#[cfg(test)]
mod symbol_mv_coverage {
    use super::{apply_plan_transactional, language_name, run_mv_command, MvOptions, MvPlan};

    #[test]
    fn touch_symbol_mv_public_api() {
        use crate::Language;

        assert_eq!(language_name(Language::Python), "python");
        assert_eq!(language_name(Language::Rust), "rust");
        let plan = MvPlan {
            files: vec![],
            edits: vec![],
        };
        let _ = apply_plan_transactional(&plan);
        let opts = MvOptions {
            query: "x".into(),
            new_name: "y".into(),
            paths: vec![],
            to: None,
            dry_run: true,
            json: false,
            lang_filter: None,
            ignore: vec![],
        };
        let _ = run_mv_command(opts);
    }
}
