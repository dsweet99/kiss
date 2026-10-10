use super::{LanguageMayWork, VcsWorkspace, git_plan_request, language_paths_may_work};
use crate::bin_cli::args::TestInvocation;
use crate::test_git::TestChangeMode;
use crate::test_runner::target_request::{GitFocus, TargetFocus, request_from_focus};
use crate::test_runner::test_mode_fixtures::dry_run_cmd_args;
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

#[test]
fn git_plan_request_reads_the_branch_from_the_focus() {
    let cases = [
        (
            GitFocus::ExplicitBase {
                branch: "dev".into(),
            },
            TestChangeMode::Base,
            None,
            None,
            Some("dev"),
        ),
        (
            GitFocus::ExplicitMain {
                branch: "trunk".into(),
            },
            TestChangeMode::Main,
            None,
            Some("trunk"),
            None,
        ),
        (
            GitFocus::ConfiguredMain {
                name: "main".into(),
            },
            TestChangeMode::Main,
            Some("main"),
            None,
            None,
        ),
    ];
    for (focus, mode, config_main, main_cli, base_cli) in cases {
        let mut args = dry_run_cmd_args(TestInvocation::Commit, &[], 1, None);
        args.main_branch_cli = Some("stale-main");
        args.base_branch_cli = Some("stale-base");
        args.config_main_branch = Some("stale-config");
        args.target_request = request_from_focus(TargetFocus::Git(focus), None, &[]);
        let req = git_plan_request(&args);
        assert_eq!(req.mode, mode);
        assert_eq!(req.config_main_branch, config_main);
        assert_eq!(req.main_branch_cli, main_cli);
        assert_eq!(req.base_branch_cli, base_cli);
    }
}
