//! One watch-cycle execution helpers.

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
    pub run_cycle: &'a mut F,
    pub run_cov: &'a mut C,
}

pub(crate) fn run_one_watch_cycle<F, C>(ctx: WatchCycleCtx<'_, F, C>) -> CycleOutcome
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>, &WatchLiveConfig) -> WatchCoverageResult,
{
    let (cycle_args, replies) = take_queued_cycle_args(ctx.live, ctx.queued);
    let capture = !replies.is_empty();
    let (test_outcome, mut captured) = run_cycle_maybe_tee(capture, &mut *ctx.run_cycle, &cycle_args);
    let test_exit = match test_outcome {
        RunTestOnceOutcome::Interrupted => {
            reply_all(&replies, EXIT_INTERRUPTED, None, None);
            return CycleOutcome::Interrupted;
        }
        RunTestOnceOutcome::Code(code) => code,
    };
    let cov_step = run_cov_step_after_tests(CovStepOpts {
        test_exit,
        dry_run: cycle_args.dry_run,
        capture,
        captured: &mut captured,
        run_cov: &mut *ctx.run_cov,
        cycle_args: &cycle_args,
        live: ctx.live,
    });
    match cov_step {
        CovStep::Interrupted => {
            reply_all(&replies, EXIT_INTERRUPTED, None, None);
            return CycleOutcome::Interrupted;
        }
        CovStep::Done {
            exit_code,
            error,
            cov_output,
        } => {
            let output = cov_output
                .filter(|s| !s.is_empty())
                .or_else(|| client_output_for_exit(exit_code, &captured));
            let error = suppress_generic_cov_error(error, output.as_ref());
            reply_all(&replies, exit_code, error, output);
        }
    }
    if let Some(msg) =
        drain_into_machine(ctx.source, ctx.filter, ctx.machine, ctx.repo_root, Duration::ZERO)
    {
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
        cov_output: Option<String>,
    },
}

struct CovStepOpts<'a, C> {
    test_exit: i32,
    dry_run: bool,
    capture: bool,
    captured: &'a mut String,
    run_cov: &'a mut C,
    cycle_args: &'a RunTestCmdArgs<'a>,
    live: &'a WatchLiveConfig,
}

fn run_cycle_maybe_tee<F>(
    capture: bool,
    run_cycle: &mut F,
    cycle_args: &RunTestCmdArgs<'_>,
) -> (RunTestOnceOutcome, String)
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
{
    if capture {
        #[cfg(unix)]
        {
            return super::client_report::tee_stdout(|| (run_cycle)(clone_args(cycle_args)));
        }
    }
    ((run_cycle)(clone_args(cycle_args)), String::new())
}

fn run_cov_step_after_tests<C>(opts: CovStepOpts<'_, C>) -> CovStep
where
    C: FnMut(&RunTestCmdArgs<'_>, &WatchLiveConfig) -> WatchCoverageResult,
{
    if opts.test_exit != 0 || opts.dry_run {
        return CovStep::Done {
            exit_code: opts.test_exit,
            error: None,
            cov_output: None,
        };
    }
    let cov = if opts.capture {
        #[cfg(unix)]
        {
            let (cov, cov_out) =
                super::client_report::tee_stdout(|| (opts.run_cov)(opts.cycle_args, opts.live));
            opts.captured.push_str(&cov_out);
            cov
        }
        #[cfg(not(unix))]
        {
            (opts.run_cov)(opts.cycle_args, opts.live)
        }
    } else {
        (opts.run_cov)(opts.cycle_args, opts.live)
    };
    if cov.interrupted {
        CovStep::Interrupted
    } else {
        CovStep::Done {
            exit_code: cov.exit_code,
            error: cov.error,
            cov_output: cov.output,
        }
    }
}

fn client_output_for_exit(exit_code: i32, captured: &str) -> Option<String> {
    if exit_code == 0 || captured.is_empty() {
        return None;
    }
    #[cfg(unix)]
    {
        let report = super::client_report::extract_client_report(captured);
        if report.is_empty() {
            None
        } else {
            Some(report)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = captured;
        None
    }
}

fn suppress_generic_cov_error(
    error: Option<String>,
    output: Option<&String>,
) -> Option<String> {
    match error {
        Some(e) if e == "coverage gate failed" && output.is_some() => None,
        other => other,
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
        replies = q.replies;
    }
    (live.cycle_args(force), replies)
}

fn reply_all(
    replies: &[SyncSender<NudgeReplyMsg>],
    exit_code: i32,
    error: Option<String>,
    output: Option<String>,
) {
    let msg = NudgeReplyMsg {
        exit_code,
        pid: std::process::id(),
        error,
        output,
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
pub(crate) use nudge_stub::NudgeRequest;
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
    }

    pub(crate) struct NudgeRequest {
        pub msg: NudgeRequestMsg,
        pub reply: SyncSender<NudgeReplyMsg>,
    }
}
