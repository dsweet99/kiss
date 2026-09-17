use crate::bin_cli::args::TestInvocation;
use crate::test_runner::force_bad::{
    apply_force_bad, prior_belongs_to_target, selector_in_target,
};
use crate::test_runner::empty_planned;

#[test]
fn apply_force_bad_noop_when_flag_off_and_merges_when_on() {
    let tmp = tempfile::tempdir().unwrap();
    let mut planned = empty_planned(tmp.path().to_path_buf(), Vec::new());
    planned.sel.python = vec!["tests/a.py::t".into()];
    let args = crate::test_runner::RunTestCmdArgs {
        invocation: TestInvocation::All,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: None,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    };
    apply_force_bad(&args, &mut planned).unwrap();
    assert!(planned.prior_failure_selectors.python.is_empty());
    let args_on = crate::test_runner::RunTestCmdArgs {
        force_bad: true,
        ..args
    };
    apply_force_bad(&args_on, &mut planned).unwrap();
    assert!(planned.prior_failure_selectors.python.is_empty());
}

#[test]
fn selector_in_target_matches_file_dir_symbol_and_nodeid() {
    assert!(selector_in_target("tests/a.py::t", "tests/a.py"));
    assert!(selector_in_target("tests/a.py::t", "tests"));
    assert!(selector_in_target("tests/a.py::t", "tests/a.py::t"));
    assert!(selector_in_target("tests/a.py::t[0]", "tests/a.py::t"));
    assert!(selector_in_target(
        "tests/a.py::C.test_m",
        "tests/a.py::C"
    ));
    assert!(!selector_in_target("tests/b.py::t", "tests/a.py"));
    assert!(!selector_in_target("tests/a.py::other", "tests/a.py::t"));
    assert!(!selector_in_target("tests/a.py::t", "src/lib.rs"));
}

#[test]
fn prior_belongs_to_target_keeps_all_on_dot_and_filters_path_targets() {
    let in_target = "tests/a.py::fail";
    let outside = "tests/b.py::fail";
    let planned = [in_target.to_string()];
    assert!(prior_belongs_to_target(
        &TestInvocation::All,
        &planned,
        outside
    ));
    let targets = TestInvocation::Targets(vec!["tests/a.py".into()]);
    assert!(prior_belongs_to_target(&targets, &planned, in_target));
    assert!(!prior_belongs_to_target(&targets, &planned, outside));
    assert!(prior_belongs_to_target(
        &TestInvocation::Commit,
        &planned,
        in_target
    ));
    assert!(!prior_belongs_to_target(
        &TestInvocation::Commit,
        &planned,
        outside
    ));
}

#[test]
fn prior_belongs_to_target_includes_path_matched_failure_absent_from_plan() {
    let targets = TestInvocation::Targets(vec!["tests/a.py".into()]);
    assert!(prior_belongs_to_target(
        &targets,
        &[],
        "tests/a.py::was_not_selected"
    ));
}

#[test]
fn prior_belongs_to_target_keeps_planned_covering_test_for_source_file() {
    let targets = TestInvocation::Targets(vec!["src/lib.rs".into()]);
    assert!(prior_belongs_to_target(
        &targets,
        &["tests/cover.py::test_lib".into()],
        "tests/cover.py::test_lib"
    ));
    assert!(!prior_belongs_to_target(
        &targets,
        &["tests/cover.py::test_lib".into()],
        "tests/other.py::unrelated"
    ));
}
