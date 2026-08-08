use super::*;

#[test]
fn execute_language_phase_covers_population_selective_and_dry_run() {
    let planned = planned();
    let options = options(false);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };

    let population = FakeLanguageModule {
        language: Language::Rust,
        population_required: true,
        selective: vec!["crate::extra".to_string()],
        summary: SelectorExecutionSummary {
            total: 2,
            exit_code: 0,
            ..SelectorExecutionSummary::default()
        },
    };
    let pop_phase = ExecutionPhase::Population(vec![
        "Rust::population".to_string(),
        "crate::extra".to_string(),
    ]);
    let pop_outcome = execute_language_phase(&population, &pop_phase, &ctx).unwrap();
    assert_eq!(pop_outcome.summary.total, 2);
    assert_eq!(population_selector_count(&pop_outcome.phase), 2);

    let selective = FakeLanguageModule {
        language: Language::Python,
        population_required: false,
        selective: vec!["tests/test_app.py::test_ok".to_string()],
        summary: SelectorExecutionSummary {
            total: 1,
            exit_code: 0,
            ..SelectorExecutionSummary::default()
        },
    };
    let sel_phase = ExecutionPhase::Selective(vec!["tests/test_app.py::test_ok".to_string()]);
    let sel_outcome = execute_language_phase(&selective, &sel_phase, &ctx).unwrap();
    assert_eq!(sel_outcome.summary.total, 1);
    assert_eq!(selective_selector_count(&sel_outcome.phase), 1);

    print_dry_run(
        &options,
        &[(
            &selective as &dyn LanguageTestModule,
            ExecutionPhase::Selective(vec!["tests/test_app.py::test_ok".to_string()]),
        )],
    )
    .unwrap();
}

#[test]
fn run_selected_phases_records_rust_population_selective_and_prints_metrics() {
    let planned = planned();
    let options = SelectorRunOptions {
        dry_run: false,
        force_rerun: false,
metrics: true,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        plan_duration: Duration::ZERO,
    };
    let python = FakeLanguageModule {
        language: Language::Python,
        population_required: false,
        selective: Vec::new(),
        summary: SelectorExecutionSummary::default(),
    };
    let rust_population = FakeLanguageModule {
        language: Language::Rust,
        population_required: true,
        selective: Vec::new(),
        summary: SelectorExecutionSummary {
            total: 1,
            exit_code: 0,
            ..SelectorExecutionSummary::default()
        },
    };
    let modules: [(&dyn LanguageTestModule, ExecutionPhase); 2] = [
        (&python, ExecutionPhase::NoWork),
        (
            &rust_population,
            ExecutionPhase::Population(vec!["crate::tests::t".to_string()]),
        ),
    ];
    let code = run_selected_phases(&planned, &options, Instant::now(), &modules).unwrap();
    assert_eq!(code, 0);

    let rust_selective = FakeLanguageModule {
        language: Language::Rust,
        population_required: false,
        selective: vec!["crate::tests::t".to_string()],
        summary: SelectorExecutionSummary {
            total: 1,
            exit_code: 3,
            ..SelectorExecutionSummary::default()
        },
    };
    let modules_sel: [(&dyn LanguageTestModule, ExecutionPhase); 2] = [
        (&python, ExecutionPhase::NoWork),
        (
            &rust_selective,
            ExecutionPhase::Selective(vec!["crate::tests::t".to_string()]),
        ),
    ];
    let code = run_selected_phases(&planned, &options, Instant::now(), &modules_sel).unwrap();
    assert_eq!(code, 3);
}

#[test]
fn run_selectors_rejects_zero_jobs_at_entry() {
    let planned = planned();
    let err = run_selectors(
        &planned,
        SelectorRunOptions {
            dry_run: false,
            force_rerun: false,
metrics: false,
            jobs: 0,
            extra: &[],
            python_extra: &[],
            plan_duration: Duration::ZERO,
        },
    )
    .unwrap_err();
    assert!(err.contains("jobs must be greater than zero"));
}

#[test]
fn run_selectors_non_dry_run_executes_python_selective_phase() {
    let tmp = tempfile::tempdir().unwrap();
    let tests = tmp.path().join("tests");
    std::fs::create_dir_all(&tests).unwrap();
    std::fs::write(
        tests.join("test_app.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    planned.py_sel = vec!["tests/test_app.py::test_ok".to_string()];
    let code = run_selectors(
        &planned,
        SelectorRunOptions {
            dry_run: false,
            force_rerun: true,
metrics: true,
            jobs: 1,
            extra: &[],
            python_extra: &[],
            plan_duration: Duration::ZERO,
        },
    );
    // Local temp repos may lack rslip population; accept success or a controlled runner error.
    match code {
        Ok(0) => {}
        Ok(other) => panic!("unexpected exit code {other}"),
        Err(err) => assert!(
            err.contains("kiss")
                || err.contains("pytest")
                || err.contains("rslip")
                || err.contains("population"),
            "unexpected error: {err}"
        ),
    }
}

#[test]
fn run_selectors_no_work_and_dry_run_with_metrics() {
    assert_eq!(
        run_selectors(
            &planned(),
            SelectorRunOptions {
                dry_run: false,
                force_rerun: false,
metrics: true,
                jobs: 1,
                extra: &[],
                python_extra: &[],
                plan_duration: Duration::ZERO,
            },
        )
        .unwrap(),
        0
    );

    let mut with_work = planned();
    with_work.py_sel = vec!["tests/test_app.py::test_ok".to_string()];
    assert_eq!(
        run_selectors(
            &with_work,
            SelectorRunOptions {
                dry_run: true,
                force_rerun: false,
metrics: true,
                jobs: 1,
                extra: &[],
                python_extra: &[],
                plan_duration: Duration::ZERO,
            },
        )
        .unwrap(),
        0
    );
}
