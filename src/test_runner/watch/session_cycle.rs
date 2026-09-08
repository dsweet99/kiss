use std::path::Path;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

#[cfg(unix)]
use super::control::NudgeReplyMsg;
use super::coverage::WatchCoverageResult;
use super::event_source::WatchEventSource;
use super::filter::WatchPathFilter;
use super::reload::{CycleForceFlags, WatchLiveConfig};
use super::session_idle::{QueuedCycle, drain_into_machine};
use super::settle::SettleMachine;
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
    pub run_cycle: &'a mut F,
    pub run_cov: &'a mut C,
}

pub(crate) fn run_one_watch_cycle<F, C>(ctx: WatchCycleCtx<'_, F, C>) -> CycleOutcome
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>, &WatchLiveConfig) -> WatchCoverageResult,
{
    kiss::rust_llvm_cov_runner::begin_watch_report_capture();
    let (cycle_args, replies) = take_queued_cycle_args(ctx.live, ctx.queued);
    crate::test_runner::emit_test_progress("kiss test: Starting");
    let test_outcome = (ctx.run_cycle)(clone_args(&cycle_args));
    let test_exit = match test_outcome {
        RunTestOnceOutcome::Interrupted => {
            *ctx.last_reply = Some(reply_all(
                &replies,
                EXIT_INTERRUPTED,
                None,
                kiss::rust_llvm_cov_runner::take_watch_report_capture(),
            ));
            return CycleOutcome::Interrupted;
        }
        RunTestOnceOutcome::Code(code) => code,
    };
    match run_cov_step_after_tests(CovStepOpts {
        test_exit,
        dry_run: cycle_args.dry_run,
        run_cov: &mut *ctx.run_cov,
        cycle_args: &cycle_args,
        live: ctx.live,
    }) {
        CovStep::Interrupted => {
            *ctx.last_reply = Some(reply_all(
                &replies,
                EXIT_INTERRUPTED,
                None,
                kiss::rust_llvm_cov_runner::take_watch_report_capture(),
            ));
            return CycleOutcome::Interrupted;
        }
        CovStep::Done { exit_code, error } => {
            *ctx.last_reply = Some(reply_all(
                &replies,
                exit_code,
                error,
                kiss::rust_llvm_cov_runner::take_watch_report_capture(),
            ));
        }
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

enum CovStep {
    Interrupted,
    Done {
        exit_code: i32,
        error: Option<String>,
    },
}

struct CovStepOpts<'a, C> {
    test_exit: i32,
    dry_run: bool,
    run_cov: &'a mut C,
    cycle_args: &'a RunTestCmdArgs<'a>,
    live: &'a WatchLiveConfig,
}

fn run_cov_step_after_tests<C>(opts: CovStepOpts<'_, C>) -> CovStep
where
    C: FnMut(&RunTestCmdArgs<'_>, &WatchLiveConfig) -> WatchCoverageResult,
{
    if opts.test_exit != 0 || opts.dry_run {
        return CovStep::Done {
            exit_code: opts.test_exit,
            error: None,
        };
    }
    let cov = (opts.run_cov)(opts.cycle_args, opts.live);
    if cov.interrupted {
        CovStep::Interrupted
    } else {
        CovStep::Done {
            exit_code: cov.exit_code,
            error: cov.error,
        }
    }
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
        }
        replies = q.replies;
    }
    (live.cycle_args(force), replies)
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
pub(crate) use nudge_stub::{NudgeReplyMsg, NudgeRequest};
#[cfg(not(unix))]
use nudge_stub::*;
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
        pub targets: Vec<String>,
    }

    pub(crate) struct NudgeRequest {
        pub msg: NudgeRequestMsg,
        pub reply: SyncSender<NudgeReplyMsg>,
    }
}
