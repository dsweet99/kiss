//! Empty `PlannedSelectors` for run-logic unit tests.

use std::path::PathBuf;

use crate::test_runner::language_keyed::LanguageKeyed;
use crate::test_runner::PlannedSelectors;

pub(crate) fn empty_planned_selectors(repo_root: PathBuf) -> PlannedSelectors {
    PlannedSelectors {
        repo_root,
        sel: LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        population_required: LanguageKeyed {
            python: false,
            rust: false,
        },
        rust_source_paths: Vec::new(),
        rust_vcs_source_paths: 0,
        rust_snapshot_delta_modified: 0,
        rust_snapshot_delta_structural: false,
        prior_failure_selectors: LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        coverage_decision_engine_used: true,
        rust_selection_basis: Default::default(),
        ignore: Vec::new(),
        workspace_files_fingerprint: None,
        skip_python_index_rebuild_after_selective: false,
    }
}
