use super::*;

#[test]
fn force_rerun_does_not_make_rust_population_required() {
    let mut planned = planned();
    planned.source_paths.rust = vec![PathBuf::from("src/lib.rs")];
    planned.sel.rust = vec!["crate::tests::test_selected".to_string()];
    let options = options(true);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };

    assert!(matches!(
        execution_phase(&execution_module_rust(&planned), &ctx).unwrap(),
        ExecutionPhase::Selective(_)
    ));
}

#[test]
fn force_all_population_helper_keeps_targets_selective() {
    let mut planned = planned();
    planned.sel.python = vec!["tests/a.py::only".to_string()];
    let args = crate::test_runner::RunTestCmdArgs {
        doubles: None,
        invocation: crate::bin_cli::args::TestInvocation::Targets(vec!["tests/a.py::only".into()]),
        target_request: crate::test_runner::target_request::operands_request(
            &["tests/a.py::only".into()],
            Some(Language::Python),
            &[],
        ),
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: true,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    };
    crate::test_runner::apply_force_all_population(&args, &mut planned);
    assert!(!planned.population_required.python);
    let options = options(true);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };
    assert_eq!(
        execution_phase(&execution_module_python(&planned), &ctx).unwrap(),
        ExecutionPhase::Selective(vec!["tests/a.py::only".to_string()])
    );
}

#[test]
fn force_all_population_helper_sets_population_for_all() {
    let mut planned = planned();
    planned.sel.python = vec!["tests/a.py::only".to_string()];
    let args = crate::test_runner::RunTestCmdArgs {
        doubles: None,
        invocation: crate::bin_cli::args::TestInvocation::All,
        target_request: crate::test_runner::target_request::workspace_request(
            Some(Language::Python),
            &[],
        ),
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: true,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    };
    crate::test_runner::apply_force_all_population(&args, &mut planned);
    assert!(planned.population_required.python);
}

#[test]
fn rust_population_phase_uses_discover_universe() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(
        src.join("lib.rs"),
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() {} }\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();

    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    planned.source_paths.rust = vec![src.join("lib.rs")];
    planned.population_required.rust = true;
    let options = options(false);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };
    let module = execution_module_rust(&planned);
    let discovered = LanguagePlanner::discover_universe(&module).unwrap();
    let discovered_ids: Vec<String> = discovered.into_iter().map(|s| s.id).collect();

    assert!(matches!(
        execution_phase(&module, &ctx).unwrap(),
        ExecutionPhase::Population(selectors) if selectors == discovered_ids
    ));
}

#[test]
fn rust_population_phase_also_executes_current_selected_tests() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(
        src.join("lib.rs"),
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() {} }\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();

    let selected = "external_selected::test_case".to_string();
    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    planned.source_paths.rust = vec![src.join("lib.rs")];
    planned.population_required.rust = true;
    planned.sel.rust = vec![selected.clone()];
    let options = options(false);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };

    let ExecutionPhase::Population(selectors) =
        execution_phase(&execution_module_rust(&planned), &ctx).unwrap()
    else {
        panic!("rust should execute a population phase");
    };

    assert!(selectors.contains(&"tests::gets_value".to_string()));
    assert!(selectors.contains(&selected));
}

#[test]
fn rust_dry_run_is_population_xor_selective() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(
        src.join("lib.rs"),
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() {} }\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();

    let selective = vec!["crate::selective_test".to_string()];
    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    planned.sel.rust = selective.clone();
    planned.population_required.rust = true;
    let options = options(false);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };

    assert!(matches!(
        execution_phase(&execution_module_rust(&planned), &ctx).unwrap(),
        ExecutionPhase::Population(_)
    ));

    planned.population_required.rust = false;
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };
    assert_eq!(
        execution_phase(&execution_module_rust(&planned), &ctx).unwrap(),
        ExecutionPhase::Selective(selective)
    );
}

#[test]
fn python_population_phase_uses_discover_universe() {
    let tmp = tempfile::tempdir().unwrap();
    let tests = tmp.path().join("tests");
    std::fs::create_dir(&tests).unwrap();
    let test_app = tests.join("test_app.py");
    std::fs::write(&test_app, "def test_value():\n    assert True\n").unwrap();

    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    planned.population_required.python = true;
    let options = options(false);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };
    let module = execution_module_python(&planned);
    let discovered = LanguagePlanner::discover_universe(&module).unwrap();
    let discovered_ids: Vec<String> = discovered.into_iter().map(|s| s.id).collect();
    assert!(!discovered_ids.is_empty());
    assert_eq!(
        discovered_ids,
        vec!["tests/test_app.py::test_value".to_string()]
    );

    assert!(matches!(
        execution_phase(&module, &ctx).unwrap(),
        ExecutionPhase::Population(selectors) if selectors == discovered_ids
    ));
}

#[test]
fn python_population_reuses_planned_selectors_without_rediscovery() {
    let mut planned = planned();
    planned.repo_root = PathBuf::from("/nonexistent/repo/for/planned-population");
    planned.population_required.python = true;
    planned.sel.python = vec!["tests/cached.py::test_cached".to_string()];
    let options = options(false);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };
    assert_eq!(
        execution_phase(&execution_module_python(&planned), &ctx).unwrap(),
        ExecutionPhase::Population(vec!["tests/cached.py::test_cached".to_string()])
    );
}

#[test]
fn selective_execution_does_not_require_discoverable_repo() {
    let mut planned = planned();
    planned.repo_root = PathBuf::from("/nonexistent/repo/for/selective");
    planned.sel.python = vec!["tests/test_app.py::test_ok".to_string()];
    planned.sel.rust = vec!["crate::tests::test_ok".to_string()];
    let options = options(false);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };

    assert_eq!(
        execution_phase(&execution_module_python(&planned), &ctx).unwrap(),
        ExecutionPhase::Selective(vec!["tests/test_app.py::test_ok".to_string()])
    );
    assert_eq!(
        execution_phase(&execution_module_rust(&planned), &ctx).unwrap(),
        ExecutionPhase::Selective(vec!["crate::tests::test_ok".to_string()])
    );
}

#[test]
fn planned_selectors_carry_population_decisions_without_selector_vectors() {
    let planned = PlannedSelectors {
        repo_root: PathBuf::from("."),
        sel: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        population_required: crate::test_runner::language_keyed::LanguageKeyed {
            python: true,
            rust: true,
        },
        source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed { python: 0, rust: 0 },
        prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        selection_engine_used: true,
        selection_basis: Default::default(),
        ignore: Vec::new(),
        workspace_files_fingerprint: None,
        skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
    };
    assert!(planned.population_required.python);
    assert!(planned.population_required.rust);
}

#[test]
fn population_selector_count_comes_from_execution_phase() {
    let phase = ExecutionPhase::Population(vec![
        "tests/test_app.py::test_a".to_string(),
        "tests/test_app.py::test_b".to_string(),
    ]);
    assert_eq!(population_selector_count(&phase), 2);
    assert_eq!(
        population_selector_count(&ExecutionPhase::Selective(vec!["x".to_string()])),
        0
    );
    assert_eq!(population_selector_count(&ExecutionPhase::NoWork), 0);
}

#[test]
fn empty_module_runs_return_default_summaries_without_spawning() {
    let planned = planned();
    let options = options(false);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };

    let outcome: LanguagePhaseOutcome = execute_language_phase(
        &execution_module_python(&planned),
        &ExecutionPhase::NoWork,
        &ctx,
    )
    .unwrap();
    assert_eq!(outcome.summary, SelectorExecutionSummary::default());
    assert!(matches!(outcome.phase, ExecutionPhase::NoWork));
}

#[test]
fn python_module_reports_python_policy() {
    assert_eq!(Language::Python, kiss::Language::Python);
}

#[test]
fn rust_module_reports_rust_policy() {
    assert_eq!(Language::Rust, kiss::Language::Rust);
}

#[test]
fn language_phase_outcome_carries_phase_summary_and_timings() {
    let outcome = LanguagePhaseOutcome {
        phase: ExecutionPhase::NoWork,
        summary: SelectorExecutionSummary::default(),
        phase_duration: Duration::ZERO,
        index_rebuild_duration: Duration::ZERO,
    };

    assert!(matches!(outcome.phase, ExecutionPhase::NoWork));
    assert_eq!(outcome.summary.total, 0);
    assert_eq!(outcome.phase_duration, Duration::ZERO);
    assert_eq!(outcome.index_rebuild_duration, Duration::ZERO);
}

#[test]
fn python_outcome_records_index_rebuild_duration_in_metrics() {
    let planned = planned();
    let options = options(false);
    let mut metrics =
        LocalRubricMetrics::new(&planned, &options, 0, false, 0, 0, Default::default());
    let outcome = LanguagePhaseOutcome {
        phase: ExecutionPhase::Selective(vec!["tests/test_app.py::test_ok".to_string()]),
        summary: SelectorExecutionSummary {
            total: 1,
            cache_hits: 0,
            cache_misses: 1,
            cache_unstored: 0,
            failed: 0,
            exit_code: 0,
            ..SelectorExecutionSummary::default()
        },
        phase_duration: Duration::from_millis(7),
        index_rebuild_duration: Duration::from_millis(3),
    };

    let module = FakeLanguageModule {
        language: Language::Python,
        population_required: false,
        selective: Vec::new(),
        summary: SelectorExecutionSummary::default(),
    };
    record_language_outcome(&mut metrics, &module, outcome);

    assert_eq!(metrics.python.summary.total, 1);
    assert_eq!(metrics.python.duration, Duration::from_millis(7));
    assert_eq!(
        metrics.python_index_rebuild_duration,
        Duration::from_millis(3)
    );
}
