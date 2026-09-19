use std::path::Path;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

#[cfg(unix)]
use super::control::NudgeReplyMsg;
use super::coverage::WatchCoverageResult;
use super::event_source::WatchEventSource;
use super::filter::WatchPathFilter;
use super::reload::{CycleForceFlags, WatchLiveConfig};
use super::session_idle::{LastReplies, drain_into_machine, QueuedCycle};
use super::settle::SettleMachine;
use kiss::rust_llvm_cov_runner::{WatchSuiteReport, WatchSuiteTotals};

use crate::bin_cli::args::TestInvocation;
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
}

fn merge_cycle_suite(
    suite: &mut WatchSuiteReport,
    lines: &[String],
    totals: Option<&WatchSuiteTotals>,
    scoped: bool,
) {
    if scoped {
        suite.merge_lines(lines);
        return;
    }
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
    let lang_nudge = ctx
        .queued
        .as_ref()
        .is_some_and(|q| q.lang_filter.is_some());
    apply_queued_filters(ctx.live, ctx.queued);
    let live = &*ctx.live;
    let (cycle_args, replies) = take_queued_cycle_args(live, ctx.queued);
    let target_scoped = !matches!(cycle_args.invocation, TestInvocation::All);
    let reuse_cov = lang_nudge && ctx.last_reply.get(None).is_some();
    let report = crate::test_runner::run_kiss_test_report(
        crate::test_runner::clone_run_args(&cycle_args),
        &mut *ctx.run_cycle,
        |args| {
            if reuse_cov {
                WatchCoverageResult::ok(0)
            } else {
                (ctx.run_cov)(args, live)
            }
        },
    );
    merge_cycle_suite(
        ctx.suite,
        &report.lines,
        report.totals.as_ref(),
        target_scoped || cycle_args.lang_filter.is_some(),
    );
    if report.interrupted {
        store_interrupted_reply(ctx.last_reply, &replies, ctx.suite, cycle_args.lang_filter);
        return CycleOutcome::Interrupted;
    }
    let waiter = reply_all(
        &replies,
        report.exit_code,
        report.error.clone(),
        ensure_green_gate_line(report.exit_code, report.output),
    );
    store_cycle_replies(
        ctx.last_reply,
        ctx.suite,
        &cycle_args,
        live.lang_filter,
        report.exit_code,
        report.error,
        waiter,
    );
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

pub(crate) fn apply_queued_filters(live: &mut WatchLiveConfig, queued: &Option<QueuedCycle>) {
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

fn store_interrupted_reply(
    last: &mut LastReplies,
    replies: &[SyncSender<NudgeReplyMsg>],
    suite: &kiss::rust_llvm_cov_runner::WatchSuiteReport,
    lang_filter: Option<kiss::Language>,
) {
    let msg = reply_all(
        replies,
        EXIT_INTERRUPTED,
        None,
        nonempty_report(suite.format()),
    );
    last.store(None, msg.clone());
    if let Some(lang) = lang_filter {
        last.store(Some(lang), msg);
    }
}

fn store_cycle_replies(
    last: &mut LastReplies,
    suite: &mut kiss::rust_llvm_cov_runner::WatchSuiteReport,
    cycle_args: &RunTestCmdArgs<'_>,
    live_lang: Option<kiss::Language>,
    exit_code: i32,
    error: Option<String>,
    waiter: NudgeReplyMsg,
) {
    let target_scoped = !matches!(cycle_args.invocation, TestInvocation::All);
    if let Some(lang) = cycle_args.lang_filter {
        last.store(Some(lang), waiter);
        let foreign_lang = cycle_args.lang_filter != live_lang;
        if last.get(None).is_some() && target_scoped {
            return;
        }
        if last.get(None).is_some() && foreign_lang {
            store_full_suite_reply(last, suite, exit_code, error);
            return;
        }
    } else if target_scoped && last.get(None).is_some() {
        return;
    }
    if exit_code == 0 {
        suite.merge_lines(&["NO VIOLATIONS".into()]);
    }
    store_full_suite_reply(last, suite, exit_code, error);
}

fn store_full_suite_reply(
    last: &mut LastReplies,
    suite: &kiss::rust_llvm_cov_runner::WatchSuiteReport,
    exit_code: i32,
    error: Option<String>,
) {
    let bilingual = NudgeReplyMsg {
        exit_code: kiss::rust_llvm_cov_runner::merge_watch_exit(exit_code, suite.test_exit_code()),
        pid: std::process::id(),
        error,
        output: nonempty_report(suite.format()),
    };
    last.store(None, bilingual.clone());
    last.store_named_language_slices(suite, &bilingual);
}

fn nonempty_report(text: String) -> Option<String> {
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn ensure_green_gate_line(exit_code: i32, output: Option<String>) -> Option<String> {
    let text = output.unwrap_or_default();
    if exit_code != 0 {
        return nonempty_report(text);
    }
    if text.contains("NO VIOLATIONS") || text.contains("VIOLATION:") {
        return nonempty_report(text);
    }
    if let Some((head, summary)) = text.rsplit_once('\n')
        && summary.contains(" passed · ")
    {
        return nonempty_report(format!("{head}\nNO VIOLATIONS\n{summary}"));
    }
    if text.is_empty() {
        return Some("NO VIOLATIONS".into());
    }
    nonempty_report(format!("{text}\nNO VIOLATIONS"))
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

#[cfg(test)]
mod ensure_green_gate_line_tests {
    use super::{WatchSuiteReport, WatchSuiteTotals, ensure_green_gate_line, merge_cycle_suite};

    #[test]
    fn inserts_no_violations_before_summary_on_green() {
        let out = ensure_green_gate_line(
            0,
            Some("PASS (cached): 3 selectors\n✓ 3 passed · 0 failed · 0 timed out · 0.1s total · 0s max pass".into()),
        )
        .unwrap();
        assert!(out.contains("NO VIOLATIONS"), "{out}");
        let clean = out.find("NO VIOLATIONS").unwrap();
        let summary = out.find("✓ 3 passed").unwrap();
        assert!(clean < summary, "{out}");
    }

    #[test]
    fn leaves_failing_output_unchanged() {
        let raw = "✗ 0 passed · 1 failed · 0 timed out · 0.1s total · 0s max pass";
        let out = ensure_green_gate_line(1, Some(raw.into())).unwrap();
        assert_eq!(out, raw);
        assert!(!out.contains("NO VIOLATIONS"));
    }

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
            false,
        );
        assert_eq!(suite.passed(), 11816);
        assert_eq!(suite.failed(), 2);
        assert_eq!(suite.timed_out(), 1);
        let recap = suite.format();
        assert!(recap.contains("11816 passed") && recap.contains("69.33s total"), "{recap}");
        assert!(!recap.contains("2633 passed"), "{recap}");
    }

    #[test]
    fn scoped_merge_does_not_apply_totals() {
        let mut suite = WatchSuiteReport::default();
        merge_cycle_suite(
            &mut suite,
            &[
                "PASS (cached): 11816 selectors".into(),
                "✓ 11816 passed · 0 failed · 0 timed out · 1s total · 0s max pass".into(),
            ],
            Some(&WatchSuiteTotals {
                passed: 11816,
                failed: 0,
                timed_out: 0,
                total_label: "1s".into(),
                max_pass_label: "0s".into(),
            }),
            false,
        );
        merge_cycle_suite(
            &mut suite,
            &[
                "PASS: tests/a.py::t (0.01s)".into(),
                "✓ 1 passed · 0 failed · 0 timed out · 0.01s total · 0s max pass".into(),
            ],
            Some(&WatchSuiteTotals {
                passed: 1,
                failed: 0,
                timed_out: 0,
                total_label: "0.01s".into(),
                max_pass_label: "0s".into(),
            }),
            true,
        );
        assert_eq!(suite.passed(), 11816, "{}", suite.format());
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
    }

    pub(crate) struct NudgeRequestMsg {
        pub force: bool,
        pub force_bad: bool,
        pub metrics: bool,
        pub invocation: crate::test_runner::watch::nudge_kind::NudgeInvocation,
        pub targets: Vec<String>,
        pub lang: Option<String>,
        pub ignore: Vec<String>,
        pub extra: Vec<String>,
        pub python_extra: Vec<String>,
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
