use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(unix)]
use super::control::{NudgeReplyMsg, NudgeRequest};
use super::coverage::WatchCoverageResult;
use super::event_source::WatchEventSource;
use super::filter::WatchPathFilter;
use super::reload::{WatchLiveConfig, WatchReloadSeed};
use super::session_cycle::{run_one_watch_cycle, CycleOutcome, WatchCycleCtx, EXIT_INTERRUPTED};
#[cfg(not(unix))]
use super::session_cycle::{NudgeReplyMsg, NudgeRequest};
use super::session_idle::{
    coalesce_nudges, force_ready_if_pending, reply_all_queued, try_reply_idle_nudge,
    wait_until_next_cycle, QueuedCycle,
};
use super::settle::SettleMachine;
use crate::test_runner::runners::clear_python_collect_memo;
use crate::test_runner::{run_test_once, RunTestCmdArgs, RunTestOnceOutcome};

#[cfg(test)]
fn watch_loop_serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[allow(unused_imports)]
pub(super) use super::session_cycle::take_queued_cycle_args;

#[allow(dead_code)]
pub(crate) fn run_watch_loop(
    args: RunTestCmdArgs<'_>,
    settle: Duration,
    repo_root: &Path,
    source: &mut dyn WatchEventSource,
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
) -> i32 {
    let mut noop_cov = |_cycle_args: &RunTestCmdArgs<'_>| WatchCoverageResult::ok(0);
    run_watch_loop_with(
        args,
        settle,
        repo_root,
        source,
        nudge_rx,
        run_test_once,
        &mut noop_cov,
    )
}

fn live_from_args_disabled(
    args: RunTestCmdArgs<'_>,
    settle: Duration,
    repo_root: &Path,
) -> WatchLiveConfig {
    let seed = WatchReloadSeed {
        cli_ignore: args.ignore.to_vec(),
        jobs_cli: Some(args.jobs),
        extra: args.extra.to_vec(),
        coverage_all: false,
        enabled: false,
        config_path: PathBuf::from(".kissconfig"),
    };
    let config_path = repo_root.join(".kissconfig");
    WatchLiveConfig::from_args(
        &args,
        settle,
        seed,
        kiss::Config::python_defaults(),
        kiss::Config::rust_defaults(),
        &config_path,
    )
}

pub(crate) fn run_watch_loop_with<F, C>(
    args: RunTestCmdArgs<'_>,
    settle: Duration,
    repo_root: &Path,
    source: &mut dyn WatchEventSource,
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
    run_cycle: F,
    mut run_cov: C,
) -> i32
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>) -> WatchCoverageResult,
{
    let live = live_from_args_disabled(args, settle, repo_root);
    run_watch_loop_live(
        live,
        repo_root,
        source,
        nudge_rx,
        run_cycle,
        |cycle, _live| run_cov(cycle),
    )
}

pub(crate) fn run_watch_loop_live<F, C>(
    mut live: WatchLiveConfig,
    repo_root: &Path,
    source: &mut dyn WatchEventSource,
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
    mut run_cycle: F,
    mut run_cov: C,
) -> i32
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>, &WatchLiveConfig) -> WatchCoverageResult,
{
    #[cfg(test)]
    let _serial = watch_loop_serial();
    let mut filter = WatchPathFilter::build_with_config(
        repo_root,
        &live.ignore,
        live.lang_filter,
        &live.invocation,
        live.watched_config_path(),
    );
    let mut machine = SettleMachine::new(live.settle);
    let mut queued: Option<QueuedCycle> = None;
    let mut last_reply = None;
    let mut suite = kiss::rust_llvm_cov_runner::WatchSuiteReport::default();
    let mut initial = true;
    loop {
        if !initial {
            clear_python_collect_memo();
        }
        initial = false;
        if let Err(msg) = live.maybe_reload(repo_root, &mut machine, &mut filter) {
            eprintln!("error: kiss test --watch: {msg}");
            return 1;
        }
        match run_one_watch_cycle(WatchCycleCtx {
            live: &live,
            queued: &mut queued,
            source,
            filter: &mut filter,
            machine: &mut machine,
            repo_root,
            last_reply: &mut last_reply,
            suite: &mut suite,
            run_cycle: &mut run_cycle,
            run_cov: &mut run_cov,
        }) {
            CycleOutcome::Interrupted => {
                coalesce_nudges(nudge_rx, &mut queued);
                let msg = last_reply.clone().unwrap_or(NudgeReplyMsg {
                    exit_code: EXIT_INTERRUPTED,
                    pid: std::process::id(),
                    error: None,
                    output: None,
                });
                reply_all_queued(&mut queued, &msg);
                return EXIT_INTERRUPTED;
            }
            CycleOutcome::Error => return 1,
            CycleOutcome::Continue => {}
        }
        coalesce_nudges(nudge_rx, &mut queued);
        if !try_reply_idle_nudge(&mut queued, last_reply.as_ref(), machine.has_pending_work())
            && queued.is_some()
        {
            force_ready_if_pending(&mut machine, repo_root);
            continue;
        }
        if let Some(code) = wait_until_next_cycle(
            source,
            &mut filter,
            &mut machine,
            repo_root,
            nudge_rx,
            &mut queued,
            last_reply.as_ref(),
        ) {
            return code;
        }
    }
}

#[cfg(test)]
#[path = "session_test.rs"]
mod tests;

#[cfg(test)]
#[path = "session_nudge_suite.rs"]
mod nudge_suite;
