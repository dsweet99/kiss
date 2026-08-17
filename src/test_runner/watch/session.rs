//! Watch session orchestration for `kiss test --watch`.

use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(unix)]
use super::control::NudgeRequest;
#[cfg(not(unix))]
use super::session_cycle::NudgeRequest;
use super::coverage::WatchCoverageResult;
use super::event_source::WatchEventSource;
use super::filter::WatchPathFilter;
use super::reload::{WatchLiveConfig, WatchReloadSeed};
use super::session_cycle::{
    CycleOutcome, EXIT_INTERRUPTED, WatchCycleCtx, run_one_watch_cycle,
};
use super::session_idle::{
    QueuedCycle, coalesce_nudges, force_ready_if_pending, wait_until_next_cycle,
};
use super::settle::SettleMachine;
use crate::test_runner::runners::clear_python_collect_memo;
use crate::test_runner::{RunTestCmdArgs, RunTestOnceOutcome, run_test_once};

// Re-export for nudge unit tests that import via `super::super::*`.
#[allow(unused_imports)]
pub(super) use super::session_cycle::take_queued_cycle_args;

#[allow(dead_code)] // used by session unit tests via `super::*`
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

/// Like `run_watch_loop`, but the cycle body and coverage step are injectable (unit tests).
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
    let mut filter =
        WatchPathFilter::build(repo_root, &live.ignore, live.lang_filter, &live.invocation);
    let mut machine = SettleMachine::new(live.settle);
    let mut queued: Option<QueuedCycle> = None;
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
            run_cycle: &mut run_cycle,
            run_cov: &mut run_cov,
        }) {
            CycleOutcome::Interrupted => return EXIT_INTERRUPTED,
            CycleOutcome::Error => return 1,
            CycleOutcome::Continue => {}
        }
        coalesce_nudges(nudge_rx, &mut queued);
        if queued.is_some() {
            force_ready_if_pending(&mut machine, repo_root);
            continue;
        }
        if let Some(code) =
            wait_until_next_cycle(source, &mut filter, &mut machine, repo_root, nudge_rx, &mut queued)
        {
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
