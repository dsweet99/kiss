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
        source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_modified: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_structural: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        prior_failure_selectors: LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        coverage_decision_engine_used: true,
        selection_basis: Default::default(),
        ignore: Vec::new(),
        workspace_files_fingerprint: None,
        skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
    }
}
