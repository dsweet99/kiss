use super::*;
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::target_request::{operands_request, workspace_request};

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
    crate::test_runner::test_mode_fixtures::dry_run_cmd_args(
        TestInvocation::All,
        &[],
        2,
        Some(Language::Python),
    )
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
    let mut filter = WatchPathFilter::build(
        tmp.path(),
        &[],
        Some(Language::Python),
        &workspace_request(Some(Language::Python), &[]),
    );

    assert!(
        !live
            .maybe_reload(tmp.path(), &mut machine, &mut filter)
            .unwrap()
    );

    std::fs::write(
        &cfg_path,
        "[test]\ntest_coverage_threshold = 90\nwatch_settle_seconds = 2.5\nnum_jobs = 8\n",
    )
    .unwrap();

    assert!(
        live.maybe_reload(tmp.path(), &mut machine, &mut filter)
            .unwrap()
    );
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
    let mut filter = WatchPathFilter::build(
        tmp.path(),
        &[],
        Some(Language::Python),
        &workspace_request(Some(Language::Python), &[]),
    );
    live.apply_reload_from_path(&cfg_path).unwrap();
    live.kissconfig_sig = PathSignature::from_path(&cfg_path);
    live.kissconfig_digest = file_digest(&cfg_path);
    assert_eq!(live.gate_config.test_coverage_threshold, 10);

    std::fs::write(&cfg_path, b).unwrap();

    live.kissconfig_sig = PathSignature::from_path(&cfg_path);
    assert_ne!(file_digest(&cfg_path), live.kissconfig_digest);
    assert!(
        live.maybe_reload(tmp.path(), &mut machine, &mut filter)
            .unwrap()
    );
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
    let mut filter = WatchPathFilter::build(tmp.path(), &[], None, &workspace_request(None, &[]));
    std::fs::write(&cfg_path, "[test]\ntest_coverage_threshold = 99\n").unwrap();
    assert!(
        !live
            .maybe_reload(tmp.path(), &mut machine, &mut filter)
            .unwrap()
    );
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
    let host_jobs = TestSectionConfig::try_load().unwrap_or_default().num_jobs;
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
    let mut filter = WatchPathFilter::build(
        tmp.path(),
        &[],
        Some(Language::Python),
        &workspace_request(Some(Language::Python), &[]),
    );
    std::fs::write(
        &cfg_path,
        "[test]\ntest_coverage_threshold = 12\nwatch_settle_seconds = 1.5\n",
    )
    .unwrap();
    assert!(
        live.maybe_reload(tmp.path(), &mut machine, &mut filter)
            .unwrap()
    );
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
    let mut filter = WatchPathFilter::build(
        tmp.path(),
        &[],
        Some(Language::Python),
        &workspace_request(Some(Language::Python), &[]),
    );
    assert!(
        live.maybe_reload(tmp.path(), &mut machine, &mut filter)
            .unwrap()
    );
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
    let mut filter = WatchPathFilter::build(
        tmp.path(),
        &[],
        Some(Language::Python),
        &workspace_request(Some(Language::Python), &[]),
    );
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

#[test]
fn cycle_args_prefers_target_request_over_stale_targets() {
    let args = base_args();
    let live = WatchLiveConfig::from_args(
        &args,
        Duration::from_secs(1),
        seed(false),
        Config::python_defaults(),
        Config::rust_defaults(),
        PathBuf::from(".kissconfig").as_path(),
    );
    let request = operands_request(&["tests/a.py".into()], None, &[]);
    let cycle = live.cycle_args(CycleForceFlags {
        target_request: request,
        ..CycleForceFlags::default()
    });
    assert_eq!(
        cycle.invocation,
        TestInvocation::Targets(vec!["tests/a.py".into()])
    );
}

#[test]
fn cycle_args_sorts_operands_request_pin() {
    let args = base_args();
    let live = WatchLiveConfig::from_args(
        &args,
        Duration::from_secs(1),
        seed(false),
        Config::python_defaults(),
        Config::rust_defaults(),
        PathBuf::from(".kissconfig").as_path(),
    );
    let request = operands_request(&["z.py".into(), "a.py".into()], None, &[]);
    let cycle = live.cycle_args(CycleForceFlags {
        target_request: request,
        ..CycleForceFlags::default()
    });
    assert_eq!(
        cycle.invocation,
        TestInvocation::Targets(vec!["a.py".into(), "z.py".into()])
    );
}

#[test]
fn cycle_request_base_keeps_explicit_branch() {
    use crate::test_runner::target_request::{
        GitFocus, TargetFocus, request_from_focus, request_from_run_args,
    };
    let mut args = base_args();
    args.base_branch_cli = Some("dev");
    let live = WatchLiveConfig::from_args(
        &args,
        Duration::from_secs(1),
        seed(false),
        Config::python_defaults(),
        Config::rust_defaults(),
        PathBuf::from(".kissconfig").as_path(),
    );
    let cycle = live.cycle_args(CycleForceFlags {
        target_request: request_from_focus(
            TargetFocus::Git(GitFocus::ExplicitBase {
                branch: "dev".into(),
            }),
            None,
            &[],
        ),
        ..CycleForceFlags::default()
    });
    assert!(matches!(
        request_from_run_args(&cycle).focus,
        TargetFocus::Git(GitFocus::ExplicitBase { branch }) if branch == "dev"
    ));
}

#[test]
fn cycle_request_targets_uses_operands_request() {
    use crate::test_runner::target_request::{TargetFocus, request_from_run_args};
    let args = base_args();
    let live = WatchLiveConfig::from_args(
        &args,
        Duration::from_secs(1),
        seed(false),
        Config::python_defaults(),
        Config::rust_defaults(),
        PathBuf::from(".kissconfig").as_path(),
    );
    let cycle = live.cycle_args(CycleForceFlags {
        target_request: operands_request(&["tests/b.py".into(), "tests/a.py".into()], None, &[]),
        ..CycleForceFlags::default()
    });
    assert!(matches!(
        request_from_run_args(&cycle).focus,
        TargetFocus::Operands(_)
    ));
}

#[test]
fn cycle_args_no_request_targets_uses_canonical_request() {
    let args = base_args();
    let live = WatchLiveConfig::from_args(
        &args,
        Duration::from_secs(1),
        seed(false),
        Config::python_defaults(),
        Config::rust_defaults(),
        PathBuf::from(".kissconfig").as_path(),
    );
    let cycle = live.cycle_args(CycleForceFlags {
        target_request: operands_request(&["tests/b.py".into(), "tests/a.py".into()], None, &[]),
        ..CycleForceFlags::default()
    });
    assert_eq!(
        cycle.invocation,
        TestInvocation::Targets(vec!["tests/a.py".into(), "tests/b.py".into()])
    );
}

#[test]
fn cycle_args_fallback_uses_live_target_request() {
    let mut args = base_args();
    args.set_invocation(TestInvocation::Targets(vec!["tests/a.py".into()]));
    let live = WatchLiveConfig::from_args(
        &args,
        Duration::from_secs(1),
        seed(false),
        Config::python_defaults(),
        Config::rust_defaults(),
        PathBuf::from(".kissconfig").as_path(),
    );
    let cycle = live.cycle_args(CycleForceFlags {
        target_request: live.target_request.clone(),
        ..CycleForceFlags::default()
    });
    assert_eq!(
        cycle.invocation,
        TestInvocation::Targets(vec!["tests/a.py".into()])
    );
}

#[test]
fn path_filter_uses_live_target_request() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
    std::fs::write(tmp.path().join("tests/a.py"), "").unwrap();
    std::fs::write(tmp.path().join("tests/b.py"), "").unwrap();
    let mut args = base_args();
    args.set_invocation(TestInvocation::Targets(vec!["tests/a.py".into()]));
    let live = WatchLiveConfig::from_args(
        &args,
        Duration::from_secs(1),
        seed(false),
        Config::python_defaults(),
        Config::rust_defaults(),
        PathBuf::from(".kissconfig").as_path(),
    );
    let filter = live.path_filter(tmp.path());
    assert!(filter.is_relevant(Path::new("tests/a.py")));
    assert!(!filter.is_relevant(Path::new("tests/b.py")));
}
