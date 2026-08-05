//! Watch session orchestration for `kiss test --watch`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::event_source::{NativeWatchEventSource, WatchEventSource};
use super::filter::WatchPathFilter;
use super::roots::resolve_watch_registrations;
use super::settle::{PathSignature, SettleMachine, SettlePoll};
use super::{apply_normalized_event, invocation_label, print_cycle_summary};
use crate::test_runner::runners::clear_python_collect_memo;
use crate::test_runner::{RunTestCmdArgs, RunTestOnceOutcome, run_test_once};

const EXIT_INTERRUPTED: i32 = 130;

pub(crate) fn run_test_watch(args: RunTestCmdArgs<'_>, settle: Duration) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => {
            eprintln!("error: kiss test: {e}");
            return 1;
        }
    };
    let repo_root = match crate::test_git::assert_git_repo(&cwd)
        .and_then(|_| crate::test_git::git_repo_root(&cwd))
    {
        Ok(root) => root,
        Err(e) => {
            eprintln!("error: kiss test requires a git repository ({e})");
            return 1;
        }
    };
    let registrations = match resolve_watch_registrations(&repo_root, &args.invocation, args.ignore) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: kiss test --watch: {e}");
            return 1;
        }
    };
    let mut source = match NativeWatchEventSource::register(&registrations) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: kiss test --watch: {e}");
            return 1;
        }
    };
    println!(
        "kiss test --watch {} (settle {}s)",
        invocation_label(&args.invocation),
        settle.as_secs_f64()
    );
    let _ = std::io::Write::flush(&mut std::io::stdout());
    run_watch_loop(args, settle, &repo_root, &mut source)
}

pub(crate) fn run_watch_loop(
    args: RunTestCmdArgs<'_>,
    settle: Duration,
    repo_root: &Path,
    source: &mut dyn WatchEventSource,
) -> i32 {
    let mut filter = WatchPathFilter::build(repo_root, args.ignore, args.lang_filter, &args.invocation);
    let mut machine = SettleMachine::new(settle);
    let mut initial = true;
    loop {
        if !initial {
            clear_python_collect_memo();
        }
        initial = false;
        match run_test_once(clone_args(&args)) {
            RunTestOnceOutcome::Interrupted => return EXIT_INTERRUPTED,
            RunTestOnceOutcome::Code(code) => {
                if code != 0 {
                    println!("kiss test --watch: waiting for changes after failure");
                }
            }
        }
        if let Some(msg) = drain_into_machine(source, &mut filter, &mut machine, repo_root, Duration::ZERO)
        {
            eprintln!("error: kiss test --watch: {msg}");
            return 1;
        }
        loop {
            match wait_for_settled_batch(source, &mut filter, &mut machine, repo_root) {
                WaitOutcome::Settled(paths) => {
                    print_cycle_summary(&paths);
                    break;
                }
                WaitOutcome::Terminal(msg) => {
                    eprintln!("error: kiss test --watch: {msg}");
                    return 1;
                }
                WaitOutcome::Continue => {}
            }
        }
    }
}

enum WaitOutcome {
    Settled(Vec<PathBuf>),
    Terminal(String),
    Continue,
}

fn wait_for_settled_batch(
    source: &mut dyn WatchEventSource,
    filter: &mut WatchPathFilter,
    machine: &mut SettleMachine,
    repo_root: &Path,
) -> WaitOutcome {
    let timeout = machine
        .deadline()
        .map(|deadline| deadline.saturating_duration_since(Instant::now()))
        .unwrap_or(Duration::from_secs(3600));
    match source.recv_timeout(timeout) {
        Ok(events) => {
            for event in events {
                if let Err(msg) = apply_normalized_event(event, filter, machine, repo_root) {
                    return WaitOutcome::Terminal(msg);
                }
            }
        }
        Err(super::event_source::RecvTimeout::Timeout) => {}
        Err(super::event_source::RecvTimeout::Disconnected(msg)) => {
            return WaitOutcome::Terminal(msg);
        }
    }
    match machine.poll(Instant::now(), |path| {
        PathSignature::from_path(&repo_root.join(path))
    }) {
        SettlePoll::Ready(paths) => {
            if paths.iter().any(|p| filter.is_ignore_file(p)) {
                *filter = WatchPathFilter::build(
                    repo_root,
                    filter.cli_ignore(),
                    filter.lang_filter(),
                    filter.invocation(),
                );
            }
            WaitOutcome::Settled(paths)
        }
        SettlePoll::Waiting | SettlePoll::Idle => WaitOutcome::Continue,
        SettlePoll::ScopeDirty => WaitOutcome::Settled(Vec::new()),
    }
}

fn drain_into_machine(
    source: &mut dyn WatchEventSource,
    filter: &mut WatchPathFilter,
    machine: &mut SettleMachine,
    repo_root: &Path,
    timeout: Duration,
) -> Option<String> {
    match source.recv_timeout(timeout) {
        Ok(events) => {
            for event in events {
                if let Err(msg) = apply_normalized_event(event, filter, machine, repo_root) {
                    return Some(msg);
                }
            }
            None
        }
        Err(super::event_source::RecvTimeout::Timeout) => None,
        Err(super::event_source::RecvTimeout::Disconnected(msg)) => Some(msg),
    }
}

fn clone_args<'a>(args: &RunTestCmdArgs<'a>) -> RunTestCmdArgs<'a> {
    RunTestCmdArgs {
        invocation: args.invocation.clone(),
        main_branch_cli: args.main_branch_cli,
        base_branch_cli: args.base_branch_cli,
        dry_run: args.dry_run,
        force_rerun: args.force_rerun,
        metrics: args.metrics,
        jobs: args.jobs,
        extra: args.extra,
        ignore: args.ignore,
        lang_filter: args.lang_filter,
        config_main_branch: args.config_main_branch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bin_cli::args::TestInvocation;
    use crate::test_runner::test_mode_fixtures::{git_in, init_git};
    use crate::test_runner::watch::event_source::{
        FakeWatchEventSource, NormalizedWatchEvent, RecvTimeout,
    };
    use std::collections::VecDeque;
    use std::env;

    struct ScriptedSource {
        steps: VecDeque<Result<Vec<NormalizedWatchEvent>, RecvTimeout>>,
    }

    impl WatchEventSource for ScriptedSource {
        fn recv_timeout(
            &mut self,
            timeout: Duration,
        ) -> Result<Vec<NormalizedWatchEvent>, RecvTimeout> {
            match self.steps.pop_front() {
                Some(Err(RecvTimeout::Timeout)) => {
                    std::thread::sleep(timeout.min(Duration::from_millis(20)));
                    Err(RecvTimeout::Timeout)
                }
                Some(other) => other,
                None => Err(RecvTimeout::Disconnected("done".into())),
            }
        }
    }

    #[test]
    fn settle_cycle_then_disconnect() {
        let _cwd = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        init_git(&tmp);
        let file = tmp.path().join("a.py");
        std::fs::write(&file, "x=1\n").unwrap();
        // Old mtime so settle age succeeds quickly.
        let _ = std::process::Command::new("touch")
            .args(["-d", "1970-01-01 00:00:01", file.to_str().unwrap()])
            .status();
        assert!(git_in(tmp.path()).args(["add", "a.py"]).status().unwrap().success());
        assert!(
            git_in(tmp.path())
                .args(["commit", "-m", "init"])
                .status()
                .unwrap()
                .success()
        );
        let orig = env::current_dir().unwrap();
        env::set_current_dir(tmp.path()).unwrap();
        let mut steps = VecDeque::new();
        steps.push_back(Err(RecvTimeout::Timeout)); // drain after first run
        steps.push_back(Ok(vec![NormalizedWatchEvent::Paths(vec![file.clone()])]));
        steps.push_back(Err(RecvTimeout::Timeout)); // settle wait
        steps.push_back(Err(RecvTimeout::Disconnected("done".into()))); // after second run drain
        let mut src = ScriptedSource { steps };
        let args = RunTestCmdArgs {
            invocation: TestInvocation::Targets(vec!["a.py".into()]),
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            force_rerun: false,
            metrics: false,
            jobs: 1,
            extra: &[],
            ignore: &[],
            lang_filter: Some(kiss::Language::Python),
            config_main_branch: None,
        };
        let code = run_watch_loop(args, Duration::from_millis(5), tmp.path(), &mut src);
        env::set_current_dir(orig).unwrap();
        assert_eq!(code, 1);
    }

    #[test]
    fn fake_disconnect_after_first_cycle() {
        let _cwd = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        init_git(&tmp);
        std::fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
        assert!(git_in(tmp.path()).args(["add", "a.py"]).status().unwrap().success());
        assert!(
            git_in(tmp.path())
                .args(["commit", "-m", "init"])
                .status()
                .unwrap()
                .success()
        );
        let orig = env::current_dir().unwrap();
        env::set_current_dir(tmp.path()).unwrap();
        let mut src = FakeWatchEventSource {
            events: vec![],
            disconnected: Some("boom".into()),
        };
        let args = RunTestCmdArgs {
            invocation: TestInvocation::Targets(vec!["a.py".into()]),
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            force_rerun: false,
            metrics: false,
            jobs: 1,
            extra: &[],
            ignore: &[],
            lang_filter: Some(kiss::Language::Python),
            config_main_branch: None,
        };
        let code = run_watch_loop(args, Duration::from_millis(5), tmp.path(), &mut src);
        env::set_current_dir(orig).unwrap();
        assert_eq!(code, 1);
    }
}
