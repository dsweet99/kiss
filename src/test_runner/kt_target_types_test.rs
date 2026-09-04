use crate::bin_cli::args::{parse_test_invocation, TestInvocation};
use crate::bin_cli::{finish_with_coverage, TestCommandArgs};
use crate::test_runner::test_mode_fixtures::{
    checkout_branch, git_in, init_git, warm_committed_rust_demo, warm_python_covering_demo,
    with_cwd,
};
use crate::test_runner::{plan_target_selectors, RunTestCmdArgs, TargetPlanKind};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn commit_seed(root: &Path) {
    assert!(git_in(root).args(["add", "-A"]).status().unwrap().success());
    assert!(git_in(root)
        .args(["commit", "-m", "seed"])
        .status()
        .unwrap()
        .success());
}

fn write_python_tree(root: &Path) {
    fs::create_dir_all(root.join("pkg")).unwrap();
    fs::create_dir_all(root.join("tests/fake_python")).unwrap();
    fs::write(root.join("pkg/__init__.py"), "").unwrap();
    fs::write(root.join("pkg/app.py"), "def value():\n    return 1\n").unwrap();
    fs::write(
        root.join("pkg/models.py"),
        "class Group:\n    def __init__(self):\n        self.n = 1\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/test_app.py"),
        "def test_value():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/test_group.py"),
        "class TestUser:\n    def test_email(self):\n        assert True\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/test_params.py"),
        "import pytest\n@pytest.mark.parametrize('n', [0])\ndef test_item(n):\n    assert n == 0\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/fake_python/test_models.py"),
        "class TestUser:\n    def test_email_format(self):\n        assert True\n",
    )
    .unwrap();
}

fn write_rust_tree(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn gets_value() { assert_eq!(super::value(), 1); }\n}\n",
    )
    .unwrap();
    fs::write(root.join("tests/smoke.rs"), "#[test]\nfn case_one() {}\n").unwrap();
}

fn python_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    init_git(&tmp);
    write_python_tree(tmp.path());
    commit_seed(tmp.path());
    tmp
}

fn rust_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    init_git(&tmp);
    write_rust_tree(tmp.path());
    commit_seed(tmp.path());
    tmp
}

fn mixed_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    init_git(&tmp);
    write_python_tree(tmp.path());
    write_rust_tree(tmp.path());
    commit_seed(tmp.path());
    tmp
}

fn workspace_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    init_git(&tmp);
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
    fs::write(tmp.path().join("lib.rs"), "fn f() {}\n").unwrap();
    commit_seed(tmp.path());
    checkout_branch(tmp.path(), "feature");
    tmp
}

fn dry_args(invocation: TestInvocation, ignore: &[String]) -> RunTestCmdArgs<'_> {
    RunTestCmdArgs {
        invocation,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore,
        lang_filter: None,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    }
}

fn dry_targets(root: &Path, targets: &[String], ignore: &[String]) -> i32 {
    let _cwd = crate::cwd_test_lock::lock();
    with_cwd(root, || {
        crate::test_runner::run_test(dry_args(TestInvocation::Targets(targets.to_vec()), ignore))
    })
}

fn dry_mode(root: &Path, invocation: TestInvocation) -> i32 {
    let _cwd = crate::cwd_test_lock::lock();
    with_cwd(root, || {
        crate::test_runner::run_test(dry_args(invocation, &[]))
    })
}

fn no_false_lang(code: i32) {
    assert_eq!(
        code, 0,
        "overlapped covering must not reject a valid TARGET with a false --lang filter"
    );
}

fn after_tests_pass_coverage(root: &Path, invocation: TestInvocation) -> i32 {
    let _cwd = crate::cwd_test_lock::lock();
    with_cwd(root, || {
        let test_cfg = kiss::TestSectionConfig::default();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        finish_with_coverage(
            &TestCommandArgs {
                invocation,
                main_branch: None,
                base_branch: None,
                dry_run: false,
                force: false,
                force_bad: false,
                metrics: false,
                coverage_all: false,
                watch: false,
                jobs: 1,
                jobs_cli: Some(1),
                ignore: &[],
                cli_ignore: &[],
                extra: &[],
                lang_filter: None,
                test_cfg: &test_cfg,
                py_config: &py,
                rs_config: &rs,
                gate_config: &gate,
                reload_kissconfig: false,
                config_path: None,
                language_tables: kiss::LanguageTablesPresent::both(),
            },
            0,
        )
    })
}

fn plan_err(root: &Path, targets: &[String]) -> String {
    let _cwd = crate::cwd_test_lock::lock();
    with_cwd(root, || {
        match plan_target_selectors(
            TargetPlanKind::Targets(targets),
            &[],
            crate::test_runner::language_keyed::LanguageKeyed {
                python: &[],
                rust: &[],
            },
            None,
            &kiss::GateConfig::default(),
        ) {
            Ok(_) => panic!("expected a reject for {targets:?}"),
            Err(err) => err,
        }
    })
}

fn reject_parse_on_fake_repo(operands: &[String]) -> String {
    let tmp = rust_repo();
    assert!(
        tmp.path().join("src/lib.rs").exists(),
        "reject cases run against fake rust sources in a temp repo"
    );
    parse_test_invocation(operands).unwrap_err()
}

#[test]
fn type_rejects_all_operand() {
    let err = reject_parse_on_fake_repo(&["all".into()]);
    assert!(err.contains("unknown test target 'all'"), "{err}");
}

#[test]
fn type_rejects_cov_operand() {
    let err = reject_parse_on_fake_repo(&["cov".into()]);
    assert!(err.contains("unknown test target 'cov'"), "{err}");
}

#[test]
fn type_rejects_mixed_dot() {
    let err = reject_parse_on_fake_repo(&[".".into(), "src/lib.rs".into()]);
    assert!(err.contains("cannot be mixed"), "{err}");
}

#[test]
fn type_rejects_mixed_reserved() {
    let err = reject_parse_on_fake_repo(&["commit".into(), "src/lib.rs".into()]);
    assert!(err.contains("reserved action 'commit'"), "{err}");
}

#[test]
fn type_rejects_missing_file() {
    let tmp = rust_repo();
    let err = plan_err(tmp.path(), &["missing.rs".into()]);
    assert!(err.contains("file not found"), "{err}");
}

#[test]
fn type_rejects_missing_symbol() {
    let tmp = rust_repo();
    let err = plan_err(tmp.path(), &["src/lib.rs::does_not_exist".into()]);
    assert!(err.contains("unresolved symbol"), "{err}");
}

#[test]
fn type_rejects_ignore_path_symbol() {
    let tmp = python_repo();
    let query = crate::test_runner::targets::resolve_target_operands(
        tmp.path(),
        &["tests/fake_python/test_models.py::TestUser.test_email_format".into()],
        None,
        &["fake_".into()],
        &[],
    )
    .unwrap_err();
    assert!(query.contains("--ignore"), "{query}");
}

fn no_post_test_gate(code: i32) {
    assert_eq!(
        code, 0,
        "post-test coverage/timing gate must not fail after tests already passed"
    );
}

#[test]
fn type_dot() {
    no_false_lang(dry_mode(workspace_repo().path(), TestInvocation::All));
    let tmp = TempDir::new().unwrap();
    warm_committed_rust_demo(&tmp);
    no_post_test_gate(after_tests_pass_coverage(tmp.path(), TestInvocation::All));
}

#[test]
fn type_commit() {
    no_false_lang(dry_mode(workspace_repo().path(), TestInvocation::Commit));
    let tmp = TempDir::new().unwrap();
    warm_committed_rust_demo(&tmp);
    no_post_test_gate(after_tests_pass_coverage(
        tmp.path(),
        TestInvocation::Commit,
    ));
}

#[test]
fn type_base() {
    no_false_lang(dry_mode(workspace_repo().path(), TestInvocation::Base));
    let tmp = TempDir::new().unwrap();
    warm_committed_rust_demo(&tmp);
    no_post_test_gate(after_tests_pass_coverage(tmp.path(), TestInvocation::Base));
}

#[test]
fn type_main() {
    no_false_lang(dry_mode(workspace_repo().path(), TestInvocation::Main));
    let tmp = TempDir::new().unwrap();
    warm_committed_rust_demo(&tmp);
    no_post_test_gate(after_tests_pass_coverage(tmp.path(), TestInvocation::Main));
}

#[test]
fn type_directory() {
    let tmp = python_repo();
    no_false_lang(dry_targets(tmp.path(), &["pkg".into()], &[]));
    let warm = TempDir::new().unwrap();
    warm_python_covering_demo(&warm);
    no_post_test_gate(after_tests_pass_coverage(
        warm.path(),
        TestInvocation::Targets(vec!["pkg".into()]),
    ));
}

#[test]
fn type_test_directory() {
    let tmp = python_repo();
    no_false_lang(dry_targets(tmp.path(), &["tests".into()], &[]));
    no_post_test_gate(after_tests_pass_coverage(
        tmp.path(),
        TestInvocation::Targets(vec!["tests".into()]),
    ));
}

#[test]
fn type_python_path() {
    no_false_lang(dry_targets(
        python_repo().path(),
        &["tests/test_app.py".into()],
        &[],
    ));
}

#[test]
fn type_python_test_function_symbol() {
    no_false_lang(dry_targets(
        python_repo().path(),
        &["tests/test_app.py::test_value".into()],
        &[],
    ));
}

#[test]
fn type_python_class_name() {
    no_false_lang(dry_targets(
        python_repo().path(),
        &["pkg/models.py::Group".into()],
        &[],
    ));
}

#[test]
fn type_python_production_class_method() {
    no_false_lang(dry_targets(
        python_repo().path(),
        &["pkg/models.py::Group.__init__".into()],
        &[],
    ));
}

#[test]
fn type_python_test_module_class_method() {
    no_false_lang(dry_targets(
        python_repo().path(),
        &["tests/test_group.py::TestUser.test_email".into()],
        &[],
    ));
}

#[test]
fn type_python_nodeid_double_colon() {
    no_false_lang(dry_targets(
        python_repo().path(),
        &["tests/test_group.py::TestUser::test_email".into()],
        &[],
    ));
}

#[test]
fn type_python_nodeid_brackets() {
    no_false_lang(dry_targets(
        python_repo().path(),
        &["tests/test_params.py::test_item[0]".into()],
        &[],
    ));
}

#[test]
fn type_rust_production_path() {
    no_false_lang(dry_targets(rust_repo().path(), &["src/lib.rs".into()], &[]));
}

#[test]
fn type_rust_production_symbol() {
    no_false_lang(dry_targets(
        rust_repo().path(),
        &["src/lib.rs::value".into()],
        &[],
    ));
}

#[test]
fn type_rust_test_file() {
    no_false_lang(dry_targets(
        rust_repo().path(),
        &["tests/smoke.rs".into()],
        &[],
    ));
}

#[test]
fn type_rust_unit_test_symbol() {
    no_false_lang(dry_targets(
        rust_repo().path(),
        &["src/lib.rs::gets_value".into()],
        &[],
    ));
}

#[test]
fn type_mixed_language_paths() {
    no_false_lang(dry_targets(
        mixed_repo().path(),
        &["src/lib.rs".into(), "tests/test_app.py".into()],
        &[],
    ));
}
