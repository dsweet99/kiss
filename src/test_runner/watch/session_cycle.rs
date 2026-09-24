use std::path::Path;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

#[cfg(unix)]
use super::control::NudgeReplyMsg;
use super::coverage::WatchCoverageResult;
use super::event_source::WatchEventSource;
use super::filter::WatchPathFilter;
use super::reload::{CycleForceFlags, WatchLiveConfig};
use super::session_idle::{LastReplies, QueuedCycle, drain_into_machine, queued_target_request};
use super::settle::SettleMachine;
use kiss::rust_llvm_cov_runner::{WatchSuiteReport, WatchSuiteTotals};

use crate::test_runner::{RunTestCmdArgs, RunTestOnceOutcome};

pub(crate) const EXIT_INTERRUPTED: i32 = 130;

pub(crate) enum CycleOutcome {
    Continue,
    Interrupted,
    Error,
}

pub(crate) struct WatchCycleCtx<'a, F, C> {
    pub live: &'a mut WatchLiveConfig,
    pub queued: &'a mut Option<QueuedCycle>,
    pub source: &'a mut dyn WatchEventSource,
    pub filter: &'a mut WatchPathFilter,
    pub machine: &'a mut SettleMachine,
    pub repo_root: &'a Path,
    pub last_reply: &'a mut LastReplies,
    pub suite: &'a mut WatchSuiteReport,
    pub run_cycle: &'a mut F,
    pub run_cov: &'a mut C,
    pub reuse_suite: bool,
}

fn merge_cycle_suite(
    suite: &mut WatchSuiteReport,
    lines: &[String],
    totals: Option<&WatchSuiteTotals>,
) {
    suite.merge_unscoped_lines(lines);
    if let Some(totals) = totals {
        suite.apply_totals(totals);
    }
}

pub(crate) fn run_one_watch_cycle<F, C>(ctx: WatchCycleCtx<'_, F, C>) -> CycleOutcome
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>, &WatchLiveConfig) -> WatchCoverageResult,
{
    crate::test_runner::emit_test_progress("kiss test: Starting");
    apply_queued_filters(ctx.live, ctx.queued);
    let live = &*ctx.live;
    let (cycle_args, replies) = take_queued_cycle_args(live, ctx.queued);
    let _ = ctx.run_cov;
    let report = crate::test_runner::kiss_report_from_ensure_outcome(
        crate::test_runner::target_request::ensure_target_report(
            Some(ctx.repo_root),
            &cycle_args,
            ctx.reuse_suite,
            true,
            |args| (*ctx.run_cycle)(args),
        ),
    );
    if !reconcile_inventory(ctx.suite, ctx.last_reply, &cycle_args) {
        return CycleOutcome::Error;
    }
    if report.interrupted {
        store_interrupted_reply(ctx.last_reply, &replies, ctx.repo_root, &cycle_args);
        return CycleOutcome::Interrupted;
    }
    merge_cycle_suite(ctx.suite, &report.lines, report.totals.as_ref());
    if !reconcile_inventory(ctx.suite, ctx.last_reply, &cycle_args) {
        return CycleOutcome::Error;
    }
    let waiter = reply_all(
        &replies,
        report.exit_code,
        report.error.clone(),
        report.output,
    );
    store_cycle_replies(ctx.last_reply, &cycle_args, waiter);
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

#[path = "session_inventory.rs"]
mod inventory;
pub(super) use inventory::reconcile_inventory;

pub(crate) fn apply_queued_filters(live: &mut WatchLiveConfig, queued: &Option<QueuedCycle>) {
    live.nudge_coverage_all = queued.as_ref().is_some_and(|q| q.coverage_all);
    match queued {
        Some(q) => live.apply_nudge_filters(
            q.lang_filter,
            q.ignore.clone(),
            q.extra.clone(),
            q.python_extra.clone(),
        ),
        None => live.clear_nudge_filters(),
    }
}

pub(crate) fn take_queued_cycle_args<'a>(
    live: &'a WatchLiveConfig,
    queued: &mut Option<QueuedCycle>,
) -> (RunTestCmdArgs<'a>, Vec<SyncSender<NudgeReplyMsg>>) {
    let mut force = CycleForceFlags::default();
    let mut replies = Vec::new();
    if let Some(mut q) = queued.take() {
        force.force_rerun = q.force;
        force.force_bad = q.force_bad;
        force.metrics = q.metrics;
        if q.unscoped_force {
            force.target_request = live.target_request.clone();
        } else {
            force.target_request = queued_target_request(&q);
        }
        replies = q.replies.into_iter().map(|(_, tx)| tx).collect();
        *queued = q.next.take().map(|b| *b);
    } else {
        force.target_request = live.target_request.clone();
    }
    (live.cycle_args(force), replies)
}

fn cycle_ensure_hit(
    repo: &Path,
    cycle_args: &RunTestCmdArgs<'_>,
) -> Option<crate::test_runner::KissTestReport> {
    crate::test_runner::kiss_report_from_ensure_query(cycle_args, Some(repo))
}

fn store_interrupted_reply(
    last: &mut LastReplies,
    replies: &[SyncSender<NudgeReplyMsg>],
    repo: &Path,
    cycle_args: &RunTestCmdArgs<'_>,
) {
    let output = cycle_ensure_hit(repo, cycle_args).and_then(|report| report.output);
    let msg = reply_all(replies, EXIT_INTERRUPTED, None, output);
    last.store(None, msg);
}

fn store_cycle_replies(
    last: &mut LastReplies,
    cycle_args: &RunTestCmdArgs<'_>,
    waiter: NudgeReplyMsg,
) {
    if !last.matches_args(cycle_args) {
        return;
    }
    last.store(None, waiter);
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
        idle_cache: Some(false),
    };
    for reply in replies {
        let _ = reply.send(msg.clone());
    }
    msg
}

#[cfg(test)]
mod merge_cycle_suite_tests {
    use super::{WatchSuiteReport, WatchSuiteTotals, merge_cycle_suite};

    #[test]
    fn unscoped_structured_totals_win_over_last_collapsed() {
        let mut suite = WatchSuiteReport::default();
        merge_cycle_suite(
            &mut suite,
            &["PASS (cached): 2633 selectors".into()],
            Some(&WatchSuiteTotals {
                passed: 11816,
                failed: 2,
                timed_out: 1,
                total_label: "69.33s".into(),
                max_pass_label: "0s".into(),
            }),
        );
        assert_eq!(suite.passed(), 11816);
        assert_eq!(suite.failed(), 2);
        assert_eq!(suite.timed_out(), 1);
        let recap = suite.format();
        assert!(
            recap.contains("11816 passed") && recap.contains("69.33s total"),
            "{recap}"
        );
        assert!(!recap.contains("2633 passed"), "{recap}");
    }
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
        pub idle_cache: Option<bool>,
    }

    pub(crate) struct NudgeRequestMsg {
        pub force: bool,
        pub force_bad: bool,
        pub metrics: bool,
        pub extra: Vec<String>,
        pub python_extra: Vec<String>,
        pub target_request: crate::test_runner::target_request::TargetRequest,
        pub coverage_all: bool,
        pub runner: String,
        pub configuration: String,
    }

    impl NudgeRequestMsg {
        pub(crate) fn lang_filter(&self) -> Option<kiss::Language> {
            None
        }
    }

    pub(crate) struct NudgeRequest {
        pub msg: NudgeRequestMsg,
        pub reply: SyncSender<NudgeReplyMsg>,
    }
}
