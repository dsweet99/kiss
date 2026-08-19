use super::*;
use crate::bin_cli::args::TestInvocation;

fn seed(enabled: bool) -> WatchReloadSeed {
    WatchReloadSeed {
        cli_ignore: Vec::new(),
        jobs_cli: None,
        extra: Vec::new(),
        coverage_all: false,
        enabled,
        config_path: PathBuf::from(".kissconfig"),
    }
}

fn base_args() -> RunTestCmdArgs<'static> {
    RunTestCmdArgs {
        invocation: TestInvocation::All,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        jobs: 2,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: Some(Language::Python),
        config_main_branch: None,
        gate_config: GateConfig::default(),
    }
}

#[test]
fn maybe_reload_updates_threshold_and_settle() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join(".kissconfig");
    std::fs::write(
        &cfg_path,
        "[test]\ntest_coverage_threshold = 0\nwatch_settle_seconds = 1.0\nnum_jobs = 2\n",
    )
    .unwrap();
    let args = base_args();
    let mut live = WatchLiveConfig::from_args(
        &args,
        Duration::from_secs(1),
        seed(true),
        Config::python_defaults(),
        Config::rust_defaults(),
        &cfg_path,
    );
    let mut machine = SettleMachine::new(Duration::from_secs(1));
    let mut filter =
        WatchPathFilter::build(tmp.path(), &[], Some(Language::Python), &TestInvocation::All);

    assert!(!live
        .maybe_reload(tmp.path(), &mut machine, &mut filter)
        .unwrap());

    std::fs::write(
        &cfg_path,
        "[test]\ntest_coverage_threshold = 90\nwatch_settle_seconds = 2.5\nnum_jobs = 8\n",
    )
    .unwrap();

    assert!(live
        .maybe_reload(tmp.path(), &mut machine, &mut filter)
        .unwrap());
    assert_eq!(live.gate_config.test_coverage_threshold, 90);
    assert!((live.settle.as_secs_f64() - 2.5).abs() < f64::EPSILON);
    assert_eq!(live.jobs, 8);
}

#[test]
fn maybe_reload_picks_up_equal_length_same_mtime_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join(".kissconfig");
    let a = "[test]\ntest_coverage_threshold = 10\nwatch_settle_seconds = 1.0\nnum_jobs = 2\n";
    let b = "[test]\ntest_coverage_threshold = 90\nwatch_settle_seconds = 1.0\nnum_jobs = 2\n";
    assert_eq!(a.len(), b.len());
    std::fs::write(&cfg_path, a).unwrap();
    let args = base_args();
    let mut live = WatchLiveConfig::from_args(
        &args,
        Duration::from_secs(1),
        seed(true),
        Config::python_defaults(),
        Config::rust_defaults(),
        &cfg_path,
    );
    let mut machine = SettleMachine::new(Duration::from_secs(1));
    let mut filter =
        WatchPathFilter::build(tmp.path(), &[], Some(Language::Python), &TestInvocation::All);
    live.apply_reload_from_path(&cfg_path).unwrap();
    live.kissconfig_sig = PathSignature::from_path(&cfg_path);
    live.kissconfig_digest = file_digest(&cfg_path);
    assert_eq!(live.gate_config.test_coverage_threshold, 10);

    std::fs::write(&cfg_path, b).unwrap();


    live.kissconfig_sig = PathSignature::from_path(&cfg_path);
    assert_ne!(file_digest(&cfg_path), live.kissconfig_digest);
    assert!(live
        .maybe_reload(tmp.path(), &mut machine, &mut filter)
        .unwrap());
    assert_eq!(live.gate_config.test_coverage_threshold, 90);
}

#[test]
fn maybe_reload_disabled_is_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join(".kissconfig");
    std::fs::write(&cfg_path, "[test]\ntest_coverage_threshold = 1\n").unwrap();
    let mut args = base_args();
    args.gate_config.test_coverage_threshold = 7;
    let mut live = WatchLiveConfig::from_args(
        &args,
        Duration::from_secs(1),
        seed(false),
        Config::python_defaults(),
        Config::rust_defaults(),
        &cfg_path,
    );
    let mut machine = SettleMachine::new(Duration::from_secs(1));
    let mut filter =
        WatchPathFilter::build(tmp.path(), &[], None, &TestInvocation::All);
    std::fs::write(&cfg_path, "[test]\ntest_coverage_threshold = 99\n").unwrap();
    assert!(!live
        .maybe_reload(tmp.path(), &mut machine, &mut filter)
        .unwrap());
    assert_eq!(live.gate_config.test_coverage_threshold, 7);
}

#[test]
fn maybe_reload_does_not_leak_cwd_num_jobs_into_watched_file() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join(".kissconfig");
    std::fs::write(
        &cfg_path,
        "[test]\ntest_coverage_threshold = 11\nwatch_settle_seconds = 1.5\n",
    )
    .unwrap();
    let host_jobs = TestSectionConfig::try_load()
        .unwrap_or_default()
        .num_jobs;
    assert!(
        host_jobs != 2,
        "precondition: host .kissconfig num_jobs must differ from session jobs=2"
    );
    let mut args = base_args();
    args.jobs = 2;
    let mut seed = seed(true);
    seed.jobs_cli = None;
    seed.config_path = cfg_path.clone();
    let mut live = WatchLiveConfig::from_args(
        &args,
        Duration::from_secs(1),
        seed,
        Config::python_defaults(),
        Config::rust_defaults(),
        &cfg_path,
    );
    assert_eq!(live.jobs, 2);
    let mut machine = SettleMachine::new(Duration::from_secs(1));
    let mut filter =
        WatchPathFilter::build(tmp.path(), &[], Some(Language::Python), &TestInvocation::All);
    std::fs::write(
        &cfg_path,
        "[test]\ntest_coverage_threshold = 12\nwatch_settle_seconds = 1.5\n",
    )
    .unwrap();
    assert!(live
        .maybe_reload(tmp.path(), &mut machine, &mut filter)
        .unwrap());
    assert_eq!(live.gate_config.test_coverage_threshold, 12);
    assert_ne!(
        live.jobs, host_jobs,
        "H3: jobs must not leak from cwd host .kissconfig ({host_jobs})"
    );
    assert_eq!(
        live.jobs,
        TestSectionConfig::default().num_jobs,
        "missing num_jobs in watched file should use defaults, not cwd"
    );
}

#[test]
fn maybe_reload_deleted_file_resets_to_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join(".kissconfig");
    std::fs::write(
        &cfg_path,
        "[test]\ntest_coverage_threshold = 12\nnum_jobs = 9\nwatch_settle_seconds = 2.0\n",
    )
    .unwrap();
    let mut seed = seed(true);
    seed.config_path = cfg_path.clone();
    seed.jobs_cli = None;
    let mut live = WatchLiveConfig::from_args(
        &base_args(),
        Duration::from_secs(1),
        seed,
        Config::python_defaults(),
        Config::rust_defaults(),
        &cfg_path,
    );
    live.apply_reload_from_path(&cfg_path).unwrap();
    live.kissconfig_sig = PathSignature::from_path(&cfg_path);
    live.kissconfig_digest = file_digest(&cfg_path);
    assert_eq!(live.gate_config.test_coverage_threshold, 12);
    assert_eq!(live.jobs, 9);

    std::fs::remove_file(&cfg_path).unwrap();
    let mut machine = SettleMachine::new(Duration::from_secs(1));
    let mut filter =
        WatchPathFilter::build(tmp.path(), &[], Some(Language::Python), &TestInvocation::All);
    assert!(live
        .maybe_reload(tmp.path(), &mut machine, &mut filter)
        .unwrap());
    assert_eq!(
        live.gate_config.test_coverage_threshold,
        GateConfig::default().test_coverage_threshold
    );
    assert_eq!(live.jobs, TestSectionConfig::default().num_jobs);
    assert!((live.settle.as_secs_f64() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn maybe_reload_invalid_threshold_does_not_silently_default() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join(".kissconfig");
    std::fs::write(
        &cfg_path,
        "[test]\ntest_coverage_threshold = 10\nnum_jobs = 2\nwatch_settle_seconds = 1.0\n",
    )
    .unwrap();
    let mut seed = seed(true);
    seed.config_path = cfg_path.clone();
    let mut live = WatchLiveConfig::from_args(
        &base_args(),
        Duration::from_secs(1),
        seed,
        Config::python_defaults(),
        Config::rust_defaults(),
        &cfg_path,
    );
    live.apply_reload_from_path(&cfg_path).unwrap();
    live.kissconfig_sig = PathSignature::from_path(&cfg_path);
    live.kissconfig_digest = file_digest(&cfg_path);
    assert_eq!(live.gate_config.test_coverage_threshold, 10);

    std::fs::write(
        &cfg_path,
        "[test]\ntest_coverage_threshold = 101\nnum_jobs = 2\nwatch_settle_seconds = 1.0\n",
    )
    .unwrap();
    let mut machine = SettleMachine::new(Duration::from_secs(1));
    let mut filter =
        WatchPathFilter::build(tmp.path(), &[], Some(Language::Python), &TestInvocation::All);
    let result = live.maybe_reload(tmp.path(), &mut machine, &mut filter);
    assert!(
        result.is_err(),
        "invalid threshold must fail reload; got Ok with threshold={}",
        live.gate_config.test_coverage_threshold
    );
    assert_eq!(
        live.gate_config.test_coverage_threshold, 10,
        "failed reload must leave the prior live gate unchanged"
    );
}
