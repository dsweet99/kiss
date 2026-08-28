use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

#[cfg(unix)]
use super::control::{NudgeRequest, WatchSessionOwner};
use super::coverage::WatchCoverageResult;
use super::event_source::{NativeWatchEventSource, WatchEventSource};
use super::reload::{WatchLiveConfig, WatchReloadSeed};
use super::roots::resolve_watch_registrations;
use super::session::run_watch_loop_live;
#[cfg(not(unix))]
use super::session_cycle::NudgeRequest;
use crate::test_runner::{RunTestCmdArgs, run_test_once};

pub(crate) fn run_test_watch(
    args: RunTestCmdArgs<'_>,
    settle: Duration,
    seed: WatchReloadSeed,
    py_config: kiss::Config,
    rs_config: kiss::Config,
    run_cov: impl FnMut(&RunTestCmdArgs<'_>, &WatchLiveConfig) -> WatchCoverageResult,
) -> i32 {
    match prepare_watch_session(&args, &seed.config_path) {
        Ok(prepared) => {
            let config_path =
                super::reload::resolve_config_path(&prepared.repo_root, &seed.config_path);
            let live =
                WatchLiveConfig::from_args(&args, settle, seed, py_config, rs_config, &config_path);
            run_native_watch(prepared, live, run_cov)
        }
        Err(code) => code,
    }
}

#[rustfmt::skip]
fn run_native_watch(
    prepared: PreparedWatch,
    live: WatchLiveConfig,
    mut run_cov: impl FnMut(&RunTestCmdArgs<'_>, &WatchLiveConfig) -> WatchCoverageResult,
) -> i32 {
    let PreparedWatch { repo_root, mut source, owner } = prepared;
    #[cfg(unix)] let nudge = Some(&owner.control.nudge_rx);
    #[cfg(not(unix))] let nudge: Option<&Receiver<NudgeRequest>> = { let _ = owner; None };
    run_prepared_loop(&repo_root, &mut source, nudge, live, &mut run_cov)
}

fn run_prepared_loop<C>(
    repo_root: &Path,
    source: &mut dyn WatchEventSource,
    nudge_rx: Option<&Receiver<NudgeRequest>>,
    live: WatchLiveConfig,
    run_cov: &mut C,
) -> i32
where
    C: FnMut(&RunTestCmdArgs<'_>, &WatchLiveConfig) -> WatchCoverageResult,
{
    run_watch_loop_live(live, repo_root, source, nudge_rx, run_test_once, run_cov)
}

struct PreparedWatch {
    repo_root: PathBuf,
    source: NativeWatchEventSource,
    #[cfg(unix)]
    owner: WatchSessionOwner,
    #[cfg(not(unix))]
    owner: (),
}

#[rustfmt::skip]
fn prepare_watch_session(
    args: &RunTestCmdArgs<'_>,
    config_path: &Path,
) -> Result<PreparedWatch, i32> {
    let cwd = std::env::current_dir().map_err(|e| { eprintln!("error: kiss test: {e}"); 1 })?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd)
        .map_err(|e| { eprintln!("error: kiss test requires a git repository ({e})"); 1 })?;
    let registrations = resolve_watch_registrations(&repo_root, &args.invocation, args.ignore)
        .map_err(|e| { eprintln!("error: kiss test --watch: {e}"); 1 })?;
    #[cfg(unix)] let owner = WatchSessionOwner::acquire(&repo_root)
        .map_err(|e| { eprintln!("error: kiss test --watch: {e}"); 1 })?;
    #[cfg(not(unix))] let owner = ();
    let source = NativeWatchEventSource::register(
        &registrations,
        &repo_root,
        &args.invocation,
        config_path,
    )
    .map_err(|e| { eprintln!("error: kiss test --watch: {e}"); 1 })?;
    Ok(PreparedWatch { repo_root, source, owner })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bin_cli::args::TestInvocation;
    use crate::test_runner::WatchCoverageResult;
    use crate::test_runner::test_mode_fixtures::{git_in, init_git};
    use crate::test_runner::watch::event_source::FakeWatchEventSource;
    use std::env;
    use std::path::Path;
    use std::time::Duration;

    fn py_args() -> RunTestCmdArgs<'static> {
        RunTestCmdArgs {
            invocation: TestInvocation::Targets(vec!["a.py".into()]),
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
            lang_filter: Some(kiss::Language::Python),
            config_main_branch: None,
            gate_config: kiss::GateConfig::default(),
        }
    }

    #[test]
    fn prepare_watch_session_rejects_non_git_cwd() {
        let _cwd = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        let orig = env::current_dir().unwrap();
        env::set_current_dir(tmp.path()).unwrap();
        let err = prepare_watch_session(&py_args(), Path::new(".kissconfig"));
        env::set_current_dir(orig).unwrap();
        assert!(matches!(err, Err(1)));
    }

    #[test]
    fn prepare_watch_session_in_git_repo() {
        let _cwd = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        init_git(&tmp);
        std::fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
        assert!(
            git_in(tmp.path())
                .args(["add", "a.py"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            git_in(tmp.path())
                .args(["commit", "-m", "init"])
                .status()
                .unwrap()
                .success()
        );
        let orig = env::current_dir().unwrap();
        env::set_current_dir(tmp.path()).unwrap();
        let prepared = prepare_watch_session(&py_args(), Path::new(".kissconfig")).expect("prepare");
        assert_eq!(
            prepared.repo_root.canonicalize().unwrap(),
            tmp.path().canonicalize().unwrap()
        );
        #[cfg(unix)]
        {
            assert!(super::super::control::session_file_path(tmp.path()).is_file());
        }
        drop(prepared);
        env::set_current_dir(orig).unwrap();
    }

    #[test]
    fn run_test_watch_ok_path_exits_on_test_disconnect() {
        let _cwd = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        init_git(&tmp);
        std::fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
        assert!(
            git_in(tmp.path())
                .args(["add", "a.py"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            git_in(tmp.path())
                .args(["commit", "-m", "init"])
                .status()
                .unwrap()
                .success()
        );
        let orig = env::current_dir().unwrap();
        env::set_current_dir(tmp.path()).unwrap();
        super::super::event_source::TEST_IMMEDIATE_DISCONNECT
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let seed = WatchReloadSeed {
            cli_ignore: Vec::new(),
            jobs_cli: Some(1),
            extra: Vec::new(),
            coverage_all: false,
            enabled: true,
            config_path: PathBuf::from(".kissconfig"),
        };
        let code = run_test_watch(
            py_args(),
            Duration::from_millis(5),
            seed,
            kiss::Config::python_defaults(),
            kiss::Config::rust_defaults(),
            |_a, _l| WatchCoverageResult::ok(0),
        );
        super::super::event_source::TEST_IMMEDIATE_DISCONNECT
            .store(false, std::sync::atomic::Ordering::SeqCst);
        env::set_current_dir(orig).unwrap();
        assert_eq!(code, 1);
    }

    #[test]
    fn run_prepared_loop_exits_on_disconnect() {
        let tmp = tempfile::tempdir().unwrap();
        init_git(&tmp);
        let seed = WatchReloadSeed {
            cli_ignore: Vec::new(),
            jobs_cli: Some(1),
            extra: Vec::new(),
            coverage_all: false,
            enabled: false,
            config_path: PathBuf::from(".kissconfig"),
        };
        let live = WatchLiveConfig::from_args(
            &py_args(),
            Duration::from_millis(5),
            seed,
            kiss::Config::python_defaults(),
            kiss::Config::rust_defaults(),
            &tmp.path().join(".kissconfig"),
        );
        let mut fake = FakeWatchEventSource {
            events: vec![],
            disconnected: Some("done".into()),
        };
        let mut cov = |_a: &RunTestCmdArgs<'_>, _l: &WatchLiveConfig| WatchCoverageResult::ok(0);
        let code = run_prepared_loop(tmp.path(), &mut fake, None, live, &mut cov);
        assert_eq!(code, 1);
    }

    #[test]
    fn resolve_config_path_joins_relative() {
        let root = Path::new("/repo");
        assert_eq!(
            super::super::reload::resolve_config_path(root, Path::new(".kissconfig")),
            PathBuf::from("/repo/.kissconfig")
        );
        assert_eq!(
            super::super::reload::resolve_config_path(root, Path::new("/abs/cfg")),
            PathBuf::from("/abs/cfg")
        );
    }
}
