use super::{LanguageMayWork, VcsWorkspace, language_paths_may_work};
use kiss::Language;
use std::path::PathBuf;

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
    };
    assert!(language_paths_may_work(&ws, Language::Python));
    assert!(!language_paths_may_work(&ws, Language::Rust));
}
