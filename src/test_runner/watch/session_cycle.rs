use std::path::Path;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

#[cfg(unix)]
use super::control::NudgeReplyMsg;
use super::coverage::WatchCoverageResult;
use super::event_source::WatchEventSource;
use super::filter::WatchPathFilter;
use super::reload::{CycleForceFlags, WatchLiveConfig};
use super::session_idle::{drain_into_machine, QueuedCycle};
use super::settle::SettleMachine;
use crate::bin_cli::args::TestInvocation;
use crate::test_runner::{RunTestCmdArgs, RunTestOnceOutcome};

pub(crate) const EXIT_INTERRUPTED: i32 = 130;

pub(crate) enum CycleOutcome {
    Continue,
    Interrupted,
    Error,
}

pub(crate) struct WatchCycleCtx<'a, F, C> {
    pub live: &'a WatchLiveConfig,
    pub queued: &'a mut Option<QueuedCycle>,
    pub source: &'a mut dyn WatchEventSource,
    pub filter: &'a mut WatchPathFilter,
    pub machine: &'a mut SettleMachine,
    pub repo_root: &'a Path,
    pub last_reply: &'a mut Option<NudgeReplyMsg>,
    pub suite: &'a mut kiss::rust_llvm_cov_runner::WatchSuiteReport,
    pub run_cycle: &'a mut F,
    pub run_cov: &'a mut C,
}

pub(crate) fn run_one_watch_cycle<F, C>(ctx: WatchCycleCtx<'_, F, C>) -> CycleOutcome
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>, &WatchLiveConfig) -> WatchCoverageResult,
{
    crate::test_runner::emit_test_progress("kiss test: Starting");
    let (cycle_args, replies) = take_queued_cycle_args(ctx.live, ctx.queued);
    let scoped = !matches!(cycle_args.invocation, TestInvocation::All);
    let report = crate::test_runner::run_kiss_test_report(
        crate::test_runner::clone_run_args(&cycle_args),
        &mut *ctx.run_cycle,
        |args| (ctx.run_cov)(args, ctx.live),
    );
    ctx.suite.merge_lines(&report.lines);
    if report.interrupted {
        *ctx.last_reply = Some(reply_all(
            &replies,
            EXIT_INTERRUPTED,
            None,
            nonempty_report(ctx.suite.format()),
        ));
        return CycleOutcome::Interrupted;
    }
    reply_all(
        &replies,
        report.exit_code,
        report.error.clone(),
        report.output,
    );
    if !scoped || ctx.last_reply.is_none() {
        *ctx.last_reply = Some(NudgeReplyMsg {
            exit_code: kiss::rust_llvm_cov_runner::merge_watch_exit(
                report.exit_code,
                ctx.suite.test_exit_code(),
            ),
            pid: std::process::id(),
            error: report.error,
            output: nonempty_report(ctx.suite.format()),
        });
    }
    if let Some(msg) = drain_into_machine(
        ctx.source,
        ctx.filter,
        ctx.machine,
        ctx.repo_root,
        Duration::ZERO,
    ) {
        eprintln!("error: kiss test --watch: {msg}");
        return CycleOutcome::Error;
    }
    CycleOutcome::Continue
}

pub(crate) fn take_queued_cycle_args<'a>(
    live: &'a WatchLiveConfig,
    queued: &mut Option<QueuedCycle>,
) -> (RunTestCmdArgs<'a>, Vec<SyncSender<NudgeReplyMsg>>) {
    let mut force = CycleForceFlags::default();
    let mut replies = Vec::new();
    if let Some(q) = queued.take() {
        force.force_rerun = q.force;
        force.force_bad = q.force_bad;
        force.metrics = q.metrics;
        if !q.unscoped_force {
            force.targets = q.targets;
            force.invocation = q.invocation;
        }
        replies = q.replies;
    }
    (live.cycle_args(force), replies)
}

fn nonempty_report(text: String) -> Option<String> {
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn reply_all(
    replies: &[SyncSender<NudgeReplyMsg>],
    exit_code: i32,
    error: Option<String>,
    output: Option<String>,
) -> NudgeReplyMsg {
    let output = output.filter(|s| !s.is_empty());
    let msg = NudgeReplyMsg {
        exit_code,
        pid: std::process::id(),
        error,
        output,
    };
    for reply in replies {
        let _ = reply.send(msg.clone());
    }
    msg
}

#[cfg(not(unix))]
use nudge_stub::*;
#[cfg(not(unix))]
pub(crate) use nudge_stub::{NudgeReplyMsg, NudgeRequest};
#[cfg(not(unix))]
mod nudge_stub {
    use std::sync::mpsc::SyncSender;

    #[derive(Clone)]
    pub(crate) struct NudgeReplyMsg {
        pub exit_code: i32,
        pub pid: u32,
        pub error: Option<String>,
        pub output: Option<String>,
    }

    pub(crate) struct NudgeRequestMsg {
        pub force: bool,
        pub force_bad: bool,
        pub metrics: bool,
        pub invocation: crate::test_runner::watch::nudge_kind::NudgeInvocation,
        pub targets: Vec<String>,
    }

    pub(crate) struct NudgeRequest {
        pub msg: NudgeRequestMsg,
        pub reply: SyncSender<NudgeReplyMsg>,
    }
}
