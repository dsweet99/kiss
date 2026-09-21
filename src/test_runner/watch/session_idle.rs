use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::time::{Duration, Instant};

#[cfg(unix)]
use super::control::{NudgeReplyMsg, NudgeRequest};
use super::event_source::WatchEventSource;
use super::filter::WatchPathFilter;
use super::nudge_kind::NudgeInvocation;
use super::reload::WatchLiveConfig;
#[cfg(not(unix))]
use super::session_cycle::NudgeReplyMsg;
use super::settle::{PathSignature, SettleMachine, SettlePoll};
use super::{apply_normalized_event, print_cycle_summary};

pub(super) use super::session_replies::LastReplies;

#[path = "session_idle_target.rs"]
mod target;
use target::slice_or_expand_recap;

pub(super) const NUDGE_POLL_SLICE: Duration = Duration::from_millis(100);

pub(super) struct QueuedCycle {
    pub replies: Vec<(Option<kiss::Language>, SyncSender<NudgeReplyMsg>)>,
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
    pub next: Option<Box<QueuedCycle>>,
}

impl QueuedCycle {
    pub(super) fn stamp_filter_override(&mut self, live: &WatchLiveConfig) {
        self.filter_override = (!self.extra.is_empty() && self.extra != live.extra)
            || (!self.ignore.is_empty() && self.ignore != live.ignore);
    }

    pub(super) fn wants_new_cycle(&self) -> bool {
        self.force
            || self.force_bad
            || self.metrics
            || !self.invocation.is_all()
            || self.filter_override
    }

    pub(super) fn is_target_scoped(&self) -> bool {
        !self.unscoped_force
            && (!self.targets.is_empty()
                || !self.invocation.is_all()
                || self.lang_filter.is_some())
    }

    #[cfg(unix)]
    fn can_merge(&self, msg: &super::control::NudgeRequestMsg) -> bool {
        let incoming_force = msg.force && msg.targets.is_empty() && msg.invocation.is_all();
        if incoming_force || self.unscoped_force {
            return true;
        }
        let in_lang = msg.lang_filter();
        if self.lang_filter != in_lang && (self.lang_filter.is_some() || in_lang.is_some()) {
            return false;
        }
        let in_scoped = !msg.targets.is_empty() || !msg.invocation.is_all();
        let self_scoped = !self.targets.is_empty()
            || !self.invocation.is_all()
            || self.lang_filter.is_some();
        self_scoped == in_scoped
    }

    #[cfg(unix)]
    fn from_req(req: NudgeRequest) -> Self {
        let lang_filter = req.msg.lang_filter();
        Self {
            replies: vec![(lang_filter, req.reply)],
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
            next: None,
        }
    }

    #[cfg(unix)]
    fn merge_req(&mut self, req: NudgeRequest) {
        self.force |= req.msg.force;
        self.force_bad |= req.msg.force_bad;
        self.metrics |= req.msg.metrics;
        if self.invocation.is_all() && req.msg.targets.is_empty() {
            self.invocation = req.msg.invocation;
        }
        merge_nudge_targets(self, req.msg.force, &req.msg.targets);
        merge_nudge_filters(
            self,
            req.msg.lang_filter(),
            &req.msg.ignore,
            &req.msg.extra,
            &req.msg.python_extra,
        );
        self.replies.push((req.msg.lang_filter(), req.reply));
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
    last_reply: &LastReplies,
    live: &WatchLiveConfig,
) -> Option<i32> {
    crate::test_runner::emit_test_progress("kiss test: Waiting");
    loop {
        kiss::rust_llvm_cov_runner::reap_orphaned_zombies();
        coalesce_nudges(nudge_rx, queued);
        if let Some(q) = queued.as_mut() {
            q.stamp_filter_override(live);
        }
        if try_reply_idle_nudge(queued, last_reply, machine.has_pending_work()) {
            continue;
        }
        if queued.is_some() {
            force_ready_if_pending(queued, machine, repo_root);
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
    while let Some(q) = queued.take() {
        for (_, reply) in q.replies {
            let _ = reply.send(msg.clone());
        }
        *queued = q.next.map(|b| *b);
    }
}

pub(super) fn try_reply_idle_nudge(
    queued: &mut Option<QueuedCycle>,
    last_reply: &LastReplies,
    pending_files: bool,
) -> bool {
    let mut replied = false;
    while idle_head(queued, last_reply, pending_files) {
        replied = true;
    }
    replied
}

fn idle_head(
    queued: &mut Option<QueuedCycle>,
    last_reply: &LastReplies,
    pending_files: bool,
) -> bool {
    let Some(q) = queued.as_ref() else {
        return false;
    };
    if q.wants_new_cycle() || pending_files {
        return false;
    }
    if q.replies.iter().any(|(lang, _)| last_reply.get(*lang).is_none()) {
        return false;
    }
    if !q.targets.is_empty()
        && !q.replies.iter().all(|(lang, _)| {
            last_reply
                .get(*lang)
                .and_then(|msg| msg.output.as_deref())
                .and_then(|out| slice_or_expand_recap(last_reply, out, &q.targets))
                .is_some()
        })
    {
        return false;
    }
    let Some(mut q) = queued.take() else {
        return false;
    };
    *queued = q.next.take().map(|b| *b);
    for (lang, reply) in q.replies {
        let mut last = idle_cached_reply(last_reply.get(lang).cloned().unwrap_or_default());
        if !q.targets.is_empty()
            && let Some(sliced) = last
                .output
                .as_deref()
                .and_then(|out| slice_or_expand_recap(last_reply, out, &q.targets))
        {
            last.output = Some(sliced);
            last = idle_cached_reply(last);
        }
        last.idle_cache = Some(true);
        let _ = reply.send(last);
    }
    true
}

pub(crate) fn idle_cached_reply(mut last: NudgeReplyMsg) -> NudgeReplyMsg {
    let recap = last.output.as_deref().is_some_and(|s| !s.is_empty());
    let keep_cov_gate = last.error.as_deref().is_some_and(|e| e.contains("coverage gate failed"));
    if recap && !keep_cov_gate {
        last.error = None;
    }
    let output = last.output.as_deref().unwrap_or("");
    if recap && recap_has_cached_status(output, "TIMEOUT") {
        last.exit_code = 124;
    } else if recap
        && (keep_cov_gate
            || recap_has_cached_status(output, "FAIL")
            || recap_has_cached_status(output, "VIOLATION"))
    {
        last.exit_code = 1;
    } else if recap {
        last.exit_code = 0;
    }
    last
}

pub(crate) fn oneshot_client_reply(reply: NudgeReplyMsg, waited: bool) -> NudgeReplyMsg {
    match reply.idle_cache {
        Some(true) => idle_cached_reply(reply),
        Some(false) => reply,
        None if waited => reply,
        None => idle_cached_reply(reply),
    }
}

fn recap_has_cached_status(output: &str, label: &str) -> bool {
    output.lines().any(|line| line.trim_start().starts_with(label))
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

#[cfg(unix)]
fn enqueue_nudge(queued: &mut Option<QueuedCycle>, req: NudgeRequest) {
    let Some(head) = queued.as_mut() else {
        *queued = Some(QueuedCycle::from_req(req));
        return;
    };
    let mut cur = head;
    while !cur.can_merge(&req.msg) {
        if cur.next.is_none() {
            cur.next = Some(Box::new(QueuedCycle::from_req(req)));
            return;
        }
        cur = cur.next.as_mut().unwrap();
    }
    cur.merge_req(req);
}

pub(super) fn coalesce_nudges(
    nudge_rx: Option<&std::sync::mpsc::Receiver<NudgeRequest>>,
    queued: &mut Option<QueuedCycle>,
) {
    let Some(rx) = nudge_rx else {
        return;
    };
    while let Ok(req) = rx.try_recv() {
        enqueue_nudge(queued, req);
    }
}

pub(super) fn force_ready_if_pending(
    queued: &Option<QueuedCycle>, machine: &mut SettleMachine, repo_root: &Path,
) {
    if queued.as_ref().is_some_and(QueuedCycle::is_target_scoped) {
        return;
    }
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
