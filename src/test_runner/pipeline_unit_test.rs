use super::{LanguageMayWork, VcsWorkspace, language_paths_may_work, split_jobs};
use kiss::Language;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[test]
fn jobs_split_both_languages_and_lang_filter() {
    assert_eq!(split_jobs(4, true), (2, 2));
    assert_eq!(split_jobs(4, false), (4, 4));
    assert_eq!(split_jobs(1, true), (1, 1));
}

#[test]
fn configured_jobs_are_honored_without_a_hidden_cap() {
    assert_eq!(split_jobs(48, true), (24, 24));
    assert_eq!(split_jobs(48, false), (48, 48));
    assert_eq!(split_jobs(32, true), (16, 16));
    assert_eq!(split_jobs(16, false), (16, 16));
    assert_eq!(split_jobs(8, false), (8, 8));
    assert_eq!(split_jobs(0, false), (1, 1));
}

#[test]
fn vcs_spawn_uses_paths_priors_and_cold_init() {
    assert!(
        !LanguageMayWork {
            paths: false,
            priors: false,
            cold_init: false
        }
        .yes()
    );
    assert!(
        LanguageMayWork {
            paths: true,
            priors: false,
            cold_init: false
        }
        .yes()
    );
    assert!(
        LanguageMayWork {
            paths: false,
            priors: true,
            cold_init: false
        }
        .yes()
    );
    assert!(
        LanguageMayWork {
            paths: false,
            priors: false,
            cold_init: true
        }
        .yes()
    );
    let ws = VcsWorkspace {
        repo_root: PathBuf::from("."),
        ignore_norm: Vec::new(),
        source_changed: vec![PathBuf::from("lib.py")],
        test_changed: Vec::new(),
        changed_lines: BTreeMap::new(),
    };
    assert!(language_paths_may_work(&ws, Language::Python));
    assert!(!language_paths_may_work(&ws, Language::Rust));
}
