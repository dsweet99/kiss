use std::path::Path;

use crate::bin_cli::args::TestInvocation;
use crate::test_git::TestChangeMode;
use crate::test_runner::test_mode_fixtures::dry_run_cmd_args;
use kiss::Language;

use super::adapt::{
    change_mode_from_focus, compat_matches, focus_from_invocation, is_workspace_focus,
    is_workspace_run, operand_raws, operands_request, request_from_focus, request_from_invocation,
    request_from_run_args, to_compat_invocation, workspace_request,
};
use super::canon::canonicalize_target_request;
use super::types::{GitFocus, LangFilter, OperandExpr, TargetFocus, TargetRequest};

fn operand(raw: &str) -> OperandExpr {
    OperandExpr {
        raw: raw.to_string(),
    }
}

fn operands(raws: &[&str]) -> TargetFocus {
    TargetFocus::Operands(raws.iter().map(|raw| operand(raw)).collect())
}

fn request(focus: TargetFocus) -> TargetRequest {
    TargetRequest {
        focus,
        lang: None,
        ignore: Vec::new(),
    }
}

fn canon(focus: TargetFocus, root: Option<&Path>) -> TargetRequest {
    canonicalize_target_request(request(focus), root)
}

#[test]
fn workspace_and_git_kinds_are_distinct() {
    let workspace = canon(TargetFocus::Workspace, None);
    let commit = canon(TargetFocus::Git(GitFocus::Commit), None);
    let auto_base = canon(TargetFocus::Git(GitFocus::AutomaticBase), None);
    let explicit_base = canon(
        TargetFocus::Git(GitFocus::ExplicitBase {
            branch: "develop".into(),
        }),
        None,
    );
    let default_main = canon(TargetFocus::Git(GitFocus::DefaultMain), None);
    let configured_main = canon(
        TargetFocus::Git(GitFocus::ConfiguredMain {
            name: "main".into(),
        }),
        None,
    );
    let explicit_main = canon(
        TargetFocus::Git(GitFocus::ExplicitMain {
            branch: "main".into(),
        }),
        None,
    );
    let items = [
        &workspace,
        &commit,
        &auto_base,
        &explicit_base,
        &default_main,
        &configured_main,
        &explicit_main,
    ];
    for (i, left) in items.iter().enumerate() {
        for (j, right) in items.iter().enumerate() {
            assert_eq!(left == right, i == j);
        }
    }
}

#[test]
fn operand_spelling_and_order_normalize_when_semantics_match() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/app.py"), "").unwrap();
    std::fs::write(root.join("src/lib.rs"), "").unwrap();
    let abs_lib = root.join("src/lib.rs").to_string_lossy().into_owned();
    let mixed = canon(
        operands(&[
            r"src\app.py",
            "./src/lib.rs",
            "src/app.py:value",
            abs_lib.as_str(),
            "src/app.py",
        ]),
        Some(root),
    );
    let expected = canon(
        operands(&["src/app.py", "src/app.py::value", "src/lib.rs"]),
        Some(root),
    );
    assert_eq!(mixed, expected);
}

#[test]
fn directory_is_not_an_explicit_file_union() {
    let dir = canon(operands(&["src"]), None);
    let files = canon(operands(&["src/a.py", "src/b.py"]), None);
    assert_ne!(dir, files);
}

#[test]
fn nodeid_and_member_syntax_round_trip() {
    let nodeid = canon(operands(&["tests/test_params.py::test_item[0]"]), None);
    let member = canon(
        operands(&["tests/test_group.py::TestUser::test_email"]),
        None,
    );
    let rust_member = canon(operands(&["src/lib.rs::tests.gets_value"]), None);
    assert_eq!(
        nodeid,
        canon(operands(&["tests/test_params.py::test_item[0]"]), None)
    );
    assert_ne!(nodeid, member);
    assert_ne!(member, rust_member);
}

#[test]
fn language_and_ignore_are_identity() {
    let files = operands(&["pkg/app.py"]);
    let py = TargetRequest {
        focus: files.clone(),
        lang: Some(LangFilter::Python),
        ignore: vec!["tests".into()],
    };
    let rust = TargetRequest {
        focus: files,
        lang: Some(LangFilter::Rust),
        ignore: vec!["tests".into()],
    };
    assert_ne!(
        canonicalize_target_request(py.clone(), None),
        canonicalize_target_request(rust, None)
    );
    let ignore_order = TargetRequest {
        focus: operands(&["pkg/app.py"]),
        lang: Some(LangFilter::Python),
        ignore: vec!["tests/".into(), "ops".into()],
    };
    let ignore_sorted = TargetRequest {
        focus: operands(&["pkg/app.py"]),
        lang: Some(LangFilter::Python),
        ignore: vec!["ops".into(), "tests".into()],
    };
    assert_eq!(
        canonicalize_target_request(ignore_order, None),
        canonicalize_target_request(ignore_sorted, None)
    );
}

#[test]
fn adapter_preserves_compat_kind() {
    let cases = [
        (TestInvocation::All, None, None, None),
        (TestInvocation::Commit, None, None, None),
        (TestInvocation::Base, None, None, None),
        (TestInvocation::Base, None, Some("dev"), None),
        (TestInvocation::Main, None, None, None),
        (TestInvocation::Main, Some("trunk"), None, None),
        (TestInvocation::Main, None, None, Some("main")),
        (
            TestInvocation::Targets(vec!["pkg/app.py".into()]),
            None,
            None,
            None,
        ),
    ];
    for (invocation, main_cli, base_cli, config_main) in cases {
        let mut args = dry_run_cmd_args(invocation.clone(), &[], 1, Some(Language::Python));
        args.main_branch_cli = main_cli;
        args.base_branch_cli = base_cli;
        args.config_main_branch = config_main;
        args.set_invocation(invocation.clone());
        let request = request_from_run_args(&args);
        assert!(compat_matches(&request, &invocation));
        assert!(compat_matches(&request, &to_compat_invocation(&request)));
        assert_eq!(
            request.focus,
            focus_from_invocation(&invocation, main_cli, base_cli, config_main)
        );
    }
}

#[test]
fn configured_main_is_not_default_or_explicit() {
    let configured = focus_from_invocation(&TestInvocation::Main, None, None, Some("main"));
    let default = focus_from_invocation(&TestInvocation::Main, None, None, None);
    let explicit = focus_from_invocation(&TestInvocation::Main, Some("main"), None, None);
    assert_ne!(configured, default);
    assert_ne!(configured, explicit);
    assert_ne!(default, explicit);
}

#[test]
fn change_mode_from_focus_follows_git_kind() {
    assert_eq!(
        change_mode_from_focus(&TargetFocus::Git(GitFocus::Commit)),
        TestChangeMode::Commit
    );
    assert_eq!(
        change_mode_from_focus(&TargetFocus::Git(GitFocus::AutomaticBase)),
        TestChangeMode::Base
    );
    assert_eq!(
        change_mode_from_focus(&TargetFocus::Git(GitFocus::ExplicitBase {
            branch: "dev".into(),
        })),
        TestChangeMode::Base
    );
    assert_eq!(
        change_mode_from_focus(&TargetFocus::Git(GitFocus::DefaultMain)),
        TestChangeMode::Main
    );
    assert_eq!(
        change_mode_from_focus(&TargetFocus::Workspace),
        TestChangeMode::Commit
    );
}

#[test]
fn operand_raws_are_present_only_for_operands() {
    assert_eq!(
        operand_raws(&operands(&["tests/a.py", "src/lib.rs"])),
        Some(vec!["tests/a.py".into(), "src/lib.rs".into()])
    );
    assert_eq!(operand_raws(&TargetFocus::Workspace), None);
    assert_eq!(operand_raws(&TargetFocus::Git(GitFocus::Commit)), None);
}

#[test]
fn set_lang_filter_refreshes_pinned_request() {
    let mut args = dry_run_cmd_args(TestInvocation::All, &[], 1, Some(Language::Python));
    args.set_lang_filter(None);
    let request = request_from_run_args(&args);
    assert!(is_workspace_focus(&request.focus));
    assert_eq!(request.lang, None);
}

#[test]
fn set_invocation_syncs_compat_from_request() {
    let mut args = dry_run_cmd_args(TestInvocation::All, &[], 1, None);
    args.set_invocation(TestInvocation::Targets(vec!["z.py".into(), "a.py".into()]));
    assert_eq!(
        args.invocation,
        TestInvocation::Targets(vec!["a.py".into(), "z.py".into()])
    );
    assert_eq!(args.invocation, to_compat_invocation(&args.target_request));
}

#[test]
fn request_from_focus_commit_is_git_commit() {
    let request = request_from_focus(TargetFocus::Git(GitFocus::Commit), None, &[]);
    assert!(!is_workspace_focus(&request.focus));
    assert_eq!(to_compat_invocation(&request), TestInvocation::Commit);
    let operands = operands_request(&["tests/a.py".into()], None, &[]);
    assert_eq!(
        to_compat_invocation(&operands),
        TestInvocation::Targets(vec!["tests/a.py".into()])
    );
}

#[test]
fn clone_run_args_sorts_operands_pin() {
    let mut args = dry_run_cmd_args(TestInvocation::All, &[], 1, None);
    args.invocation = TestInvocation::All;
    args.target_request = operands_request(&["z.py".into(), "a.py".into()], None, &[]);
    let cloned = crate::test_runner::clone_run_args(&args);
    assert_eq!(
        operand_raws(&request_from_run_args(&cloned).focus),
        Some(vec!["a.py".into(), "z.py".into()])
    );
    assert_eq!(
        cloned.invocation,
        TestInvocation::Targets(vec!["a.py".into(), "z.py".into()])
    );
}

#[test]
fn clone_run_args_syncs_invocation_from_request() {
    let mut args = dry_run_cmd_args(TestInvocation::All, &[], 1, None);
    args.invocation = TestInvocation::Targets(vec!["stale.py".into()]);
    args.target_request = workspace_request(None, &[]);
    let cloned = crate::test_runner::clone_run_args(&args);
    assert_eq!(cloned.invocation, TestInvocation::All);
    assert!(is_workspace_focus(&request_from_run_args(&cloned).focus));
}

#[test]
fn request_from_run_args_sorts_operands_pin() {
    let mut args = dry_run_cmd_args(TestInvocation::All, &[], 1, None);
    args.invocation = TestInvocation::All;
    args.target_request = operands_request(&["z.py".into(), "a.py".into()], None, &[]);
    let request = request_from_run_args(&args);
    assert_eq!(
        operand_raws(&request.focus),
        Some(vec!["a.py".into(), "z.py".into()])
    );
}

#[test]
fn request_from_invocation_sorts_operands() {
    let request = request_from_invocation(
        &TestInvocation::Targets(vec!["z.py".into(), "a.py".into()]),
        None,
        None,
        None,
        None,
        &[],
    );
    assert_eq!(
        operand_raws(&request.focus),
        Some(vec!["a.py".into(), "z.py".into()])
    );
}

#[test]
fn dry_run_cmd_args_pins_workspace_request() {
    let args = dry_run_cmd_args(TestInvocation::All, &[], 1, None);
    assert!(is_workspace_focus(&args.target_request.focus));
    assert!(is_workspace_focus(&request_from_run_args(&args).focus));
}

#[test]
fn dry_run_cmd_args_syncs_invocation_from_request() {
    let args = dry_run_cmd_args(
        TestInvocation::Targets(vec!["z.py".into(), "a.py".into()]),
        &[],
        1,
        None,
    );
    assert_eq!(
        args.invocation,
        TestInvocation::Targets(vec!["a.py".into(), "z.py".into()])
    );
    assert_eq!(args.invocation, to_compat_invocation(&args.target_request));
}

#[test]
fn bind_prefers_pinned_request_over_stale_invocation() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::TempDir::new().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    let restore = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let mut args = dry_run_cmd_args(TestInvocation::All, &[], 1, None);
    args.invocation = TestInvocation::Targets(vec!["stale.py".into()]);
    args.target_request = workspace_request(None, &[]);
    let result = super::bind::bind_and_prepare(&args);
    std::env::set_current_dir(restore).unwrap();
    assert!(
        !matches!(result, Err(err) if err.contains("adapter mismatch")),
        "pinned Workspace must bind"
    );
}

#[test]
fn request_from_run_args_prefers_pinned_target_request() {
    let mut args = dry_run_cmd_args(TestInvocation::All, &[], 1, None);
    args.invocation = TestInvocation::Targets(vec!["stale.py".into()]);
    args.target_request = workspace_request(None, &[]);
    let request = request_from_run_args(&args);
    assert!(is_workspace_focus(&request.focus));
}

#[test]
fn workspace_request_is_workspace_focus() {
    let request = workspace_request(Some(Language::Rust), &["target".into()]);
    assert!(is_workspace_focus(&request.focus));
    assert_eq!(request.lang, Some(LangFilter::Rust));
    assert_eq!(request.ignore, vec!["target".to_string()]);
}

#[test]
fn is_workspace_focus_is_true_only_for_workspace() {
    assert!(is_workspace_focus(&TargetFocus::Workspace));
    assert!(!is_workspace_focus(&TargetFocus::Git(GitFocus::Commit)));
    assert!(!is_workspace_focus(&operands(&["tests/a.py"])));
}

#[test]
fn is_workspace_run_is_true_only_for_all() {
    let all = dry_run_cmd_args(TestInvocation::All, &[], 1, None);
    let path = dry_run_cmd_args(
        TestInvocation::Targets(vec!["tests/a.py".into()]),
        &[],
        1,
        None,
    );
    let commit = dry_run_cmd_args(TestInvocation::Commit, &[], 1, None);
    assert!(is_workspace_run(&all));
    assert!(!is_workspace_run(&path));
    assert!(!is_workspace_run(&commit));
}

#[test]
fn lang_filter_maps_both_languages() {
    assert_eq!(
        LangFilter::from_language(Language::Python).to_language(),
        Language::Python
    );
    assert_eq!(
        LangFilter::from_language(Language::Rust).to_language(),
        Language::Rust
    );
}
