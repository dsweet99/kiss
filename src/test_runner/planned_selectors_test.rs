use super::*;
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::test_mode_fixtures::empty_planned_selectors;
use kiss::Language;

fn args(
    invocation: TestInvocation,
    force: bool,
    lang: Option<Language>,
) -> RunTestCmdArgs<'static> {
    RunTestCmdArgs {
        doubles: None,
        invocation: invocation.clone(),
        target_request: crate::test_runner::target_request::request_from_invocation(
            &invocation,
            None,
            None,
            None,
            lang,
            &[],
        ),
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: false,
        force_rerun: force,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    }
}

#[test]
fn force_and_cold_helpers_cover_language_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let mut planned = empty_planned_selectors(tmp.path().to_path_buf());
    planned.sel.python = vec!["t.py::test_a".into()];
    planned.sel.rust = vec!["crate::t".into()];

    let cold = args(TestInvocation::Base, false, None);
    assert!(should_force_cold_initialization(&cold, tmp.path()));
    assert!(!should_force_cold_initialization(
        &args(TestInvocation::Commit, false, None),
        tmp.path()
    ));
    assert!(!should_force_cold_initialization(
        &args(TestInvocation::All, false, None),
        tmp.path()
    ));
    apply_cold_initialization_population(&cold, &mut planned);
    assert!(planned.population_required.python);
    assert!(planned.population_required.rust);

    planned.population_required.python = false;
    planned.population_required.rust = false;
    apply_force_all_population(
        &args(TestInvocation::All, true, Some(Language::Python)),
        &mut planned,
    );
    assert!(planned.population_required.python);
    apply_force_all_population(
        &args(TestInvocation::All, true, Some(Language::Rust)),
        &mut planned,
    );
    assert!(planned.population_required.rust);
    planned.population_required.python = false;
    planned.population_required.rust = false;
    apply_force_all_population(&args(TestInvocation::All, true, None), &mut planned);
    assert!(planned.population_required.python && planned.population_required.rust);
    apply_force_all_population(&args(TestInvocation::Commit, true, None), &mut planned);
    apply_force_all_population(&args(TestInvocation::All, false, None), &mut planned);
    let mut empty = empty_planned_selectors(tmp.path().to_path_buf());
    apply_force_all_population(&args(TestInvocation::All, true, None), &mut empty);
    assert!(!empty.population_required.python);
    assert!(!empty.population_required.rust);
}
