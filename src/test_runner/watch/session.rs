//! Watch session orchestration for `kiss test --watch`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::time::{Duration, Instant};

#[cfg(unix)]
use super::control::{NudgeReplyMsg, NudgeRequest, WatchSessionOwner};
use super::event_source::{NativeWatchEventSource, WatchEventSource};
use super::filter::WatchPathFilter;
use super::roots::resolve_watch_registrations;
use super::settle::{PathSignature, SettleMachine, SettlePoll};
use super::{apply_normalized_event, print_cycle_summary};
use crate::test_runner::runners::clear_python_collect_memo;
use crate::test_runner::{RunTestCmdArgs, RunTestOnceOutcome, run_test_once};

const EXIT_INTERRUPTED: i32 = 130;
const NUDGE_POLL_SLICE: Duration = Duration::from_millis(100);

struct QueuedCycle {
    replies: Vec<SyncSender<NudgeReplyMsg>>,
    force: bool,
    force_bad: bool,
    metrics: bool,
}

pub(crate) fn run_test_watch(args: RunTestCmdArgs<'_>, settle: Duration) -> i32 {
    match prepare_watch_session(&args) {
        Ok(PreparedWatch {
            repo_root,
            mut source,
            owner,
        }) => {
            #[cfg(unix)]
            {
                run_watch_loop(
                    args,
                    settle,
                    &repo_root,
                    &mut source,
                    Some(&owner.control.nudge_rx),
                )
            }
            #[cfg(not(unix))]
            {
                let _ = owner;
                run_watch_loop(args, settle, &repo_root, &mut source, None)
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

pub(crate) fn run_watch_loop(
    args: RunTestCmdArgs<'_>,
    settle: Duration,
    repo_root: &Path,
    source: &mut dyn WatchEventSource,
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
) -> i32 {
    run_watch_loop_with(args, settle, repo_root, source, nudge_rx, run_test_once)
}

/// Like `run_watch_loop`, but the cycle body is injectable (unit tests).
pub(crate) fn run_watch_loop_with<F>(
    args: RunTestCmdArgs<'_>,
    settle: Duration,
    repo_root: &Path,
    source: &mut dyn WatchEventSource,
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
    mut run_cycle: F,
) -> i32
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
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
        match run_one_watch_cycle(
            &args,
            &mut queued,
            source,
            &mut filter,
            &mut machine,
            repo_root,
            &mut run_cycle,
        ) {
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

fn run_one_watch_cycle<F>(
    args: &RunTestCmdArgs<'_>,
    queued: &mut Option<QueuedCycle>,
    source: &mut dyn WatchEventSource,
    filter: &mut WatchPathFilter,
    machine: &mut SettleMachine,
    repo_root: &Path,
    run_cycle: &mut F,
) -> CycleOutcome
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
{
    let (cycle_args, replies) = take_queued_cycle_args(args, queued);
    let exit_code = match run_cycle(cycle_args) {
        RunTestOnceOutcome::Interrupted => {
            reply_all(&replies, EXIT_INTERRUPTED);
            return CycleOutcome::Interrupted;
        }
        RunTestOnceOutcome::Code(code) => code,
    };
    reply_all(&replies, exit_code);
    if let Some(msg) = drain_into_machine(source, filter, machine, repo_root, Duration::ZERO) {
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

fn wait_until_next_cycle(
    source: &mut dyn WatchEventSource,
    filter: &mut WatchPathFilter,
    machine: &mut SettleMachine,
    repo_root: &Path,
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
    queued: &mut Option<QueuedCycle>,
) -> Option<i32> {
    // Once per idle/settle wait (not each nudge poll slice).
    crate::test_runner::emit_test_progress("kiss test: Waiting");
    loop {
        coalesce_nudges(nudge_rx, queued);
        if queued.is_some() {
            force_ready_if_pending(machine, repo_root);
            return None;
        }
        match wait_for_settled_batch(source, filter, machine, repo_root, nudge_rx) {
            WaitOutcome::Settled(paths) => {
                print_cycle_summary(&paths);
                return None;
            }
            WaitOutcome::Terminal(msg) => {
                eprintln!("error: kiss test --watch: {msg}");
                return Some(1);
            }
            WaitOutcome::Continue => {}
        }
    }
}

fn reply_all(replies: &[SyncSender<NudgeReplyMsg>], exit_code: i32) {
    let msg = NudgeReplyMsg {
        exit_code,
        pid: std::process::id(),
    };
    for reply in replies {
        let _ = reply.send(msg.clone());
    }
}

fn coalesce_nudges(
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
    queued: &mut Option<QueuedCycle>,
) {
    let Some(rx) = nudge_rx else {
        return;
    };
    while let Ok(req) = rx.try_recv() {
        match queued {
            Some(q) => {
                q.force |= req.msg.force;
                q.force_bad |= req.msg.force_bad;
                q.metrics |= req.msg.metrics;
                q.replies.push(req.reply);
            }
            None => {
                *queued = Some(QueuedCycle {
                    replies: vec![req.reply],
                    force: req.msg.force,
                    force_bad: req.msg.force_bad,
                    metrics: req.msg.metrics,
                });
            }
        }
    }
}

fn force_ready_if_pending(machine: &mut SettleMachine, repo_root: &Path) {
    let _ = machine.force_ready(Instant::now(), |path| {
        PathSignature::from_path(&repo_root.join(path))
    });
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
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
) -> WaitOutcome {
    // Cap idle/settle waits so the outer loop can coalesce control nudges promptly.
    // Do not try_recv here: that would consume a nudge before coalesce_nudges runs.
    let settle_timeout = machine
        .deadline()
        .map(|deadline| deadline.saturating_duration_since(Instant::now()))
        .unwrap_or(Duration::from_secs(3600));
    let timeout = if nudge_rx.is_some() {
        settle_timeout.min(NUDGE_POLL_SLICE)
    } else {
        settle_timeout
    };

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

/// Shared helpers for unit tests that need stub nudge types on all targets.
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
#[path = "session_nudge_test.rs"]
mod nudge_tests;
