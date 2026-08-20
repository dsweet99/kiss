use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::time::{Duration, Instant};

#[cfg(unix)]
use super::control::{NudgeReplyMsg, NudgeRequest};
use super::event_source::WatchEventSource;
use super::filter::WatchPathFilter;
use super::settle::{PathSignature, SettleMachine, SettlePoll};
use super::{apply_normalized_event, print_cycle_summary};

pub(super) const NUDGE_POLL_SLICE: Duration = Duration::from_millis(100);

pub(super) struct QueuedCycle {
    pub replies: Vec<SyncSender<NudgeReplyMsg>>,
    pub force: bool,
    pub force_bad: bool,
    pub metrics: bool,
}

impl QueuedCycle {
    fn wants_new_cycle(&self) -> bool {
        self.force || self.force_bad || self.metrics
    }
}

pub(super) enum WaitOutcome {
    Settled(Vec<PathBuf>),
    Terminal(String),
    Continue,
}

pub(super) fn wait_until_next_cycle(
    source: &mut dyn WatchEventSource,
    filter: &mut WatchPathFilter,
    machine: &mut SettleMachine,
    repo_root: &Path,
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
    queued: &mut Option<QueuedCycle>,
    last_reply: Option<&NudgeReplyMsg>,
) -> Option<i32> {
    crate::test_runner::emit_test_progress("kiss test: Waiting");
    loop {
        coalesce_nudges(nudge_rx, queued);
        if try_reply_idle_nudge(queued, last_reply, machine) {
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

fn try_reply_idle_nudge(
    queued: &mut Option<QueuedCycle>,
    last_reply: Option<&NudgeReplyMsg>,
    machine: &SettleMachine,
) -> bool {
    let Some(q) = queued.as_ref() else {
        return false;
    };
    if q.wants_new_cycle() || machine.has_pending_work() {
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
