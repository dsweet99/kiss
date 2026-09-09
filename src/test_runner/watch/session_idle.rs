use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::time::{Duration, Instant};

#[cfg(unix)]
use super::control::{NudgeReplyMsg, NudgeRequest};
use super::event_source::WatchEventSource;
use super::filter::WatchPathFilter;
use super::nudge_kind::NudgeInvocation;
use super::reload::WatchLiveConfig;
use super::settle::{PathSignature, SettleMachine, SettlePoll};
use super::{apply_normalized_event, print_cycle_summary};

pub(super) const NUDGE_POLL_SLICE: Duration = Duration::from_millis(100);

pub(super) struct QueuedCycle {
    pub replies: Vec<SyncSender<NudgeReplyMsg>>,
    pub force: bool,
    pub force_bad: bool,
    pub metrics: bool,
    pub invocation: NudgeInvocation,
    pub targets: Vec<String>,
    pub unscoped_force: bool,
    pub lang_filter: Option<kiss::Language>,
    pub ignore: Vec<String>,
    pub extra: Vec<String>,
    pub python_extra: Vec<String>,
    pub filter_override: bool,
}

impl QueuedCycle {
    pub(super) fn stamp_filter_override(&mut self, live: &WatchLiveConfig) {
        self.filter_override = (self.lang_filter.is_some() && self.lang_filter != live.lang_filter)
            || (!self.extra.is_empty() && self.extra != live.extra)
            || (!self.ignore.is_empty() && self.ignore != live.ignore);
    }

    pub(super) fn wants_new_cycle(&self) -> bool {
        self.force
            || self.force_bad
            || self.metrics
            || !self.targets.is_empty()
            || !self.invocation.is_all()
            || self.filter_override
    }
}

pub(super) enum WaitOutcome {
    Settled(Vec<PathBuf>),
    Terminal(String),
    Continue,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn wait_until_next_cycle(
    source: &mut dyn WatchEventSource,
    filter: &mut WatchPathFilter,
    machine: &mut SettleMachine,
    repo_root: &Path,
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
    queued: &mut Option<QueuedCycle>,
    last_reply: Option<&NudgeReplyMsg>,
    live: &WatchLiveConfig,
) -> Option<i32> {
    crate::test_runner::emit_test_progress("kiss test: Waiting");
    loop {
        coalesce_nudges(nudge_rx, queued);
        if let Some(q) = queued.as_mut() {
            q.stamp_filter_override(live);
        }
        if try_reply_idle_nudge(queued, last_reply, machine.has_pending_work()) {
            continue;
        }
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

pub(super) fn reply_all_queued(queued: &mut Option<QueuedCycle>, msg: &NudgeReplyMsg) {
    if let Some(q) = queued.take() {
        for reply in q.replies {
            let _ = reply.send(msg.clone());
        }
    }
}

pub(super) fn try_reply_idle_nudge(
    queued: &mut Option<QueuedCycle>,
    last_reply: Option<&NudgeReplyMsg>,
    pending_files: bool,
) -> bool {
    let Some(q) = queued.as_ref() else {
        return false;
    };
    if q.wants_new_cycle() || pending_files {
        return false;
    }
    let Some(last) = last_reply else {
        return false;
    };
    let Some(q) = queued.take() else {
        return false;
    };
    for reply in q.replies {
        let _ = reply.send(last.clone());
    }
    true
}

fn merge_nudge_targets(q: &mut QueuedCycle, force: bool, targets: &[String]) {
    if force && targets.is_empty() {
        q.unscoped_force = true;
        q.targets.clear();
        return;
    }
    if q.unscoped_force || targets.is_empty() {
        return;
    }
    q.targets.extend(targets.iter().cloned());
    q.targets.sort();
    q.targets.dedup();
}

fn merge_nudge_filters(
    q: &mut QueuedCycle,
    lang_filter: Option<kiss::Language>,
    ignore: &[String],
    extra: &[String],
    python_extra: &[String],
) {
    if lang_filter.is_some() {
        q.lang_filter = lang_filter;
    }
    if !ignore.is_empty() {
        q.ignore = ignore.to_vec();
    }
    if !extra.is_empty() {
        q.extra = extra.to_vec();
        q.python_extra = python_extra.to_vec();
    }
}

pub(super) fn coalesce_nudges(
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
                if q.invocation.is_all() && req.msg.targets.is_empty() {
                    q.invocation = req.msg.invocation;
                }
                merge_nudge_targets(q, req.msg.force, &req.msg.targets);
                merge_nudge_filters(
                    q,
                    req.msg.lang_filter(),
                    &req.msg.ignore,
                    &req.msg.extra,
                    &req.msg.python_extra,
                );
                q.replies.push(req.reply);
            }
            None => {
                let lang_filter = req.msg.lang_filter();
                *queued = Some(QueuedCycle {
                    replies: vec![req.reply],
                    force: req.msg.force,
                    force_bad: req.msg.force_bad,
                    metrics: req.msg.metrics,
                    invocation: req.msg.invocation,
                    unscoped_force: req.msg.force
                        && req.msg.targets.is_empty()
                        && req.msg.invocation.is_all(),
                    targets: req.msg.targets,
                    lang_filter,
                    ignore: req.msg.ignore,
                    extra: req.msg.extra,
                    python_extra: req.msg.python_extra,
                    filter_override: false,
                });
            }
        }
    }
}

pub(super) fn force_ready_if_pending(machine: &mut SettleMachine, repo_root: &Path) {
    let _ = machine.force_ready(Instant::now(), |path| {
        PathSignature::from_path(&repo_root.join(path))
    });
}

fn wait_for_settled_batch(
    source: &mut dyn WatchEventSource,
    filter: &mut WatchPathFilter,
    machine: &mut SettleMachine,
    repo_root: &Path,
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
) -> WaitOutcome {
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
                *filter = filter.rebuild();
            }
            WaitOutcome::Settled(paths)
        }
        SettlePoll::Waiting | SettlePoll::Idle => WaitOutcome::Continue,
        SettlePoll::ScopeDirty => WaitOutcome::Settled(Vec::new()),
    }
}

pub(super) fn drain_into_machine(
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
