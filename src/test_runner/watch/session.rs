//! Watch session orchestration for `kiss test --watch`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::time::Duration;

#[cfg(unix)]
use super::control::{NudgeReplyMsg, NudgeRequest, WatchSessionOwner};
use super::coverage::WatchCoverageResult;
use super::event_source::{NativeWatchEventSource, WatchEventSource};
use super::filter::WatchPathFilter;
use super::roots::resolve_watch_registrations;
use super::session_idle::{
    QueuedCycle, coalesce_nudges, drain_into_machine, force_ready_if_pending, wait_until_next_cycle,
};
use super::settle::SettleMachine;
use crate::test_runner::runners::clear_python_collect_memo;
use crate::test_runner::{RunTestCmdArgs, RunTestOnceOutcome, run_test_once};

const EXIT_INTERRUPTED: i32 = 130;

pub(crate) fn run_test_watch(
    args: RunTestCmdArgs<'_>,
    settle: Duration,
    mut run_cov: impl FnMut(&RunTestCmdArgs<'_>) -> WatchCoverageResult,
) -> i32 {
    match prepare_watch_session(&args) {
        Ok(PreparedWatch {
            repo_root,
            mut source,
            owner,
        }) => {
            #[cfg(unix)]
            {
                run_watch_loop_with(
                    args,
                    settle,
                    &repo_root,
                    &mut source,
                    Some(&owner.control.nudge_rx),
                    run_test_once,
                    &mut run_cov,
                )
            }
            #[cfg(not(unix))]
            {
                let _ = owner;
                run_watch_loop_with(
                    args,
                    settle,
                    &repo_root,
                    &mut source,
                    None,
                    run_test_once,
                    &mut run_cov,
                )
            }
        }
        Err(code) => code,
    }
}

struct PreparedWatch {
    repo_root: PathBuf,
    source: NativeWatchEventSource,
    #[cfg(unix)]
    owner: WatchSessionOwner,
    #[cfg(not(unix))]
    owner: (),
}

fn prepare_watch_session(args: &RunTestCmdArgs<'_>) -> Result<PreparedWatch, i32> {
    let cwd = std::env::current_dir().map_err(|e| {
        eprintln!("error: kiss test: {e}");
        1
    })?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd).map_err(|e| {
        eprintln!("error: kiss test requires a git repository ({e})");
        1
    })?;
    let registrations =
        resolve_watch_registrations(&repo_root, &args.invocation, args.ignore).map_err(|e| {
            eprintln!("error: kiss test --watch: {e}");
            1
        })?;

    #[cfg(unix)]
    let owner = WatchSessionOwner::acquire(&repo_root).map_err(|e| {
        eprintln!("error: kiss test --watch: {e}");
        1
    })?;
    #[cfg(not(unix))]
    let owner = ();

    let source = NativeWatchEventSource::register(&registrations).map_err(|e| {
        eprintln!("error: kiss test --watch: {e}");
        1
    })?;
    Ok(PreparedWatch {
        repo_root,
        source,
        owner,
    })
}

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

/// Like `run_watch_loop`, but the cycle body and coverage step are injectable (unit tests).
pub(crate) fn run_watch_loop_with<F, C>(
    args: RunTestCmdArgs<'_>,
    settle: Duration,
    repo_root: &Path,
    source: &mut dyn WatchEventSource,
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
    mut run_cycle: F,
    mut run_cov: C,
) -> i32
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>) -> WatchCoverageResult,
{
    let mut filter = WatchPathFilter::build(repo_root, args.ignore, args.lang_filter, &args.invocation);
    let mut machine = SettleMachine::new(settle);
    let mut queued: Option<QueuedCycle> = None;
    let mut initial = true;
    loop {
        if !initial {
            clear_python_collect_memo();
        }
        initial = false;
        match run_one_watch_cycle(WatchCycleCtx {
            args: &args,
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

enum CycleOutcome {
    Continue,
    Interrupted,
    Error,
}

struct WatchCycleCtx<'a, F, C> {
    args: &'a RunTestCmdArgs<'a>,
    queued: &'a mut Option<QueuedCycle>,
    source: &'a mut dyn WatchEventSource,
    filter: &'a mut WatchPathFilter,
    machine: &'a mut SettleMachine,
    repo_root: &'a Path,
    run_cycle: &'a mut F,
    run_cov: &'a mut C,
}

fn run_one_watch_cycle<F, C>(ctx: WatchCycleCtx<'_, F, C>) -> CycleOutcome
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>) -> WatchCoverageResult,
{
    let (cycle_args, replies) = take_queued_cycle_args(ctx.args, ctx.queued);
    let test_exit = match (ctx.run_cycle)(clone_args(&cycle_args)) {
        RunTestOnceOutcome::Interrupted => {
            reply_all(&replies, EXIT_INTERRUPTED, None);
            return CycleOutcome::Interrupted;
        }
        RunTestOnceOutcome::Code(code) => code,
    };
    let (exit_code, error) = if test_exit != 0 || cycle_args.dry_run {
        (test_exit, None)
    } else {
        let cov = (ctx.run_cov)(&cycle_args);
        if cov.interrupted {
            reply_all(&replies, EXIT_INTERRUPTED, None);
            return CycleOutcome::Interrupted;
        }
        (cov.exit_code, cov.error)
    };
    reply_all(&replies, exit_code, error);
    if let Some(msg) =
        drain_into_machine(ctx.source, ctx.filter, ctx.machine, ctx.repo_root, Duration::ZERO)
    {
        eprintln!("error: kiss test --watch: {msg}");
        return CycleOutcome::Error;
    }
    CycleOutcome::Continue
}

fn take_queued_cycle_args<'a>(
    args: &RunTestCmdArgs<'a>,
    queued: &mut Option<QueuedCycle>,
) -> (RunTestCmdArgs<'a>, Vec<SyncSender<NudgeReplyMsg>>) {
    let mut cycle_args = clone_args(args);
    let mut replies = Vec::new();
    if let Some(q) = queued.take() {
        cycle_args.force_rerun |= q.force;
        cycle_args.force_bad |= q.force_bad;
        cycle_args.metrics |= q.metrics;
        replies = q.replies;
    }
    (cycle_args, replies)
}

fn reply_all(replies: &[SyncSender<NudgeReplyMsg>], exit_code: i32, error: Option<String>) {
    let msg = NudgeReplyMsg {
        exit_code,
        pid: std::process::id(),
        error,
    };
    for reply in replies {
        let _ = reply.send(msg.clone());
    }
}

fn clone_args<'a>(args: &RunTestCmdArgs<'a>) -> RunTestCmdArgs<'a> {
    RunTestCmdArgs {
        invocation: args.invocation.clone(),
        main_branch_cli: args.main_branch_cli,
        base_branch_cli: args.base_branch_cli,
        dry_run: args.dry_run,
        force_rerun: args.force_rerun,
        force_bad: args.force_bad,
        metrics: args.metrics,
        jobs: args.jobs,
        extra: args.extra,
        python_extra: args.python_extra,
        ignore: args.ignore,
        lang_filter: args.lang_filter,
        config_main_branch: args.config_main_branch,
        gate_config: args.gate_config.clone(),
    }
}

#[cfg(not(unix))]
use nudge_stub::*;
#[cfg(not(unix))]
mod nudge_stub {
    use super::*;
    use std::sync::mpsc::SyncSender;

    #[derive(Clone)]
    pub(crate) struct NudgeReplyMsg {
        pub exit_code: i32,
        pub pid: u32,
        pub error: Option<String>,
    }

    pub(crate) struct NudgeRequestMsg {
        pub force: bool,
        pub force_bad: bool,
        pub metrics: bool,
    }

    pub(crate) struct NudgeRequest {
        pub msg: NudgeRequestMsg,
        pub reply: SyncSender<NudgeReplyMsg>,
    }
}

#[cfg(test)]
#[path = "session_test.rs"]
mod tests;

#[cfg(test)]
#[path = "session_nudge_suite.rs"]
mod nudge_suite;
