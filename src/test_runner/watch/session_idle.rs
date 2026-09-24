use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::time::{Duration, Instant};

#[cfg(unix)]
use super::control::{NudgeReplyMsg, NudgeRequest};
use super::event_source::WatchEventSource;
use super::filter::WatchPathFilter;
use super::reload::WatchLiveConfig;
#[cfg(not(unix))]
use super::session_cycle::NudgeReplyMsg;
use super::settle::{PathSignature, SettleMachine, SettlePoll};
use super::{apply_normalized_event, print_cycle_summary};

pub(super) use super::session_replies::LastReplies;

pub(super) const NUDGE_POLL_SLICE: Duration = Duration::from_millis(100);

pub(super) struct QueuedCycle {
    pub replies: Vec<(Option<kiss::Language>, SyncSender<NudgeReplyMsg>)>,
    pub force: bool,
    pub force_bad: bool,
    pub metrics: bool,
    pub targets: Vec<String>,
    pub unscoped_force: bool,
    pub lang_filter: Option<kiss::Language>,
    pub ignore: Vec<String>,
    pub extra: Vec<String>,
    pub python_extra: Vec<String>,
    pub filter_override: bool,
    pub coverage_all: bool,
    pub target_request: crate::test_runner::target_request::TargetRequest,
    pub runner: String,
    pub configuration: String,
    pub next: Option<Box<QueuedCycle>>,
}

impl QueuedCycle {
    pub(super) fn stamp_filter_override(&mut self, live: &WatchLiveConfig) {
        self.filter_override = (!self.extra.is_empty() && self.extra != live.extra)
            || (!self.python_extra.is_empty() && self.python_extra != live.python_extra)
            || (!self.ignore.is_empty() && self.ignore != live.ignore);
    }

    pub(super) fn wants_new_cycle(&self) -> bool {
        self.force || self.force_bad || self.filter_override
    }

    pub(super) fn is_workspace_focus(&self) -> bool {
        crate::test_runner::target_request::is_workspace_focus(&self.target_request.focus)
    }

    #[cfg(test)]
    pub(super) fn is_target_scoped(&self) -> bool {
        !self.unscoped_force && (!self.is_workspace_focus() || self.lang_filter.is_some())
    }

    #[cfg(unix)]
    fn can_merge(&self, msg: &super::control::NudgeRequestMsg) -> bool {
        let incoming = pin_from_nudge_msg(msg);
        let in_targets = targets_from_pin(&incoming, msg);
        let incoming_force = msg.force && in_targets.is_empty() && msg_is_workspace(msg);
        if incoming_force || self.unscoped_force {
            return true;
        }
        let in_lang = msg.lang_filter();
        if self.lang_filter != in_lang && (self.lang_filter.is_some() || in_lang.is_some()) {
            return false;
        }
        if self.target_request != incoming {
            return false;
        }
        let in_scoped = !msg_is_workspace(msg) || in_lang.is_some();
        let self_scoped = !self.is_workspace_focus() || self.lang_filter.is_some();
        self_scoped == in_scoped
    }

    #[cfg(unix)]
    fn from_req(req: NudgeRequest) -> Self {
        let target_request = pin_from_nudge_msg(&req.msg);
        let lang_filter = target_request.language();
        let targets = targets_from_pin(&target_request, &req.msg);
        Self {
            replies: vec![(lang_filter, req.reply)],
            force: req.msg.force,
            force_bad: req.msg.force_bad,
            metrics: req.msg.metrics,
            unscoped_force: req.msg.force && targets.is_empty() && msg_is_workspace(&req.msg),
            targets,
            lang_filter,
            ignore: target_request.ignore.clone(),
            extra: req.msg.extra,
            python_extra: req.msg.python_extra,
            filter_override: false,
            coverage_all: req.msg.coverage_all,
            target_request,
            runner: req.msg.runner,
            configuration: req.msg.configuration,
            next: None,
        }
    }

    #[cfg(unix)]
    fn merge_req(&mut self, req: NudgeRequest) {
        let incoming = pin_from_nudge_msg(&req.msg);
        let targets = targets_from_pin(&incoming, &req.msg);
        self.force |= req.msg.force;
        self.force_bad |= req.msg.force_bad;
        self.metrics |= req.msg.metrics;
        self.coverage_all |= req.msg.coverage_all;
        if self.runner.is_empty() {
            self.runner.clone_from(&req.msg.runner);
        }
        if self.configuration.is_empty() {
            self.configuration.clone_from(&req.msg.configuration);
        }
        merge_nudge_targets(self, req.msg.force, &targets);
        merge_nudge_filters(
            self,
            incoming.language(),
            &incoming.ignore,
            &req.msg.extra,
            &req.msg.python_extra,
        );
        self.replies.push((incoming.language(), req.reply));
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
    if q.metrics {
        return reply_metrics_target_report(queued, last_reply);
    }
    reply_ready_target_report(queued, last_reply)
}

fn reply_ready_target_report(queued: &mut Option<QueuedCycle>, last_reply: &LastReplies) -> bool {
    let Some(q) = queued.as_ref() else {
        return false;
    };
    let Some(msg) = ready_target_report_reply(q, last_reply) else {
        return false;
    };
    let Some(mut q) = queued.take() else {
        return false;
    };
    *queued = q.next.take().map(|b| *b);
    for (_, reply) in q.replies {
        let _ = reply.send(msg.clone());
    }
    true
}

fn ready_target_report_reply(q: &QueuedCycle, last_reply: &LastReplies) -> Option<NudgeReplyMsg> {
    ensure_query_reply(q, last_reply, false)
}

fn reply_metrics_target_report(queued: &mut Option<QueuedCycle>, last_reply: &LastReplies) -> bool {
    let Some(q) = queued.as_ref() else {
        return false;
    };
    let msg = ensure_query_reply(q, last_reply, true).unwrap_or_else(|| NudgeReplyMsg {
        exit_code: 1,
        pid: last_reply.clone_any().map(|msg| msg.pid).unwrap_or(0),
        error: Some("incomplete evidence".into()),
        output: None,
        idle_cache: Some(true),
    });
    let Some(mut q) = queued.take() else {
        return false;
    };
    *queued = q.next.take().map(|b| *b);
    for (_, reply) in q.replies {
        let _ = reply.send(msg.clone());
    }
    true
}

fn ensure_query_reply(
    q: &QueuedCycle,
    last_reply: &LastReplies,
    allow_error: bool,
) -> Option<NudgeReplyMsg> {
    if !protocol_identity_holds(q, &last_reply.repo) {
        return None;
    }
    let request = queued_target_request(q);
    let policy = crate::test_runner::target_request::EnsurePolicy {
        dry_run: false,
        require_complete: true,
        inject_mismatch: false,
        retry_bad: false,
        coverage_all: q.coverage_all,
        assemble_only: false,
    };
    let ensured = if matches!(
        request.focus,
        crate::test_runner::target_request::TargetFocus::Git(_)
    ) {
        crate::test_runner::target_request::assemble_target_report_query(
            &last_reply.repo,
            &request,
            &policy,
            &q.extra,
        )
    } else {
        crate::test_runner::target_request::ensure_target_report_query(
            &last_reply.repo,
            &request,
            &policy,
            &q.extra,
        )
    };
    match ensured {
        Ok(crate::test_runner::target_request::Ensured::Report(report)) => {
            Some(idle_cached_reply(NudgeReplyMsg {
                exit_code: report.exit_code,
                pid: last_reply.clone_any().map(|msg| msg.pid).unwrap_or(0),
                error: None,
                output: Some(crate::test_runner::target_request::official_report_text(
                    &report,
                )),
                idle_cache: Some(true),
            }))
        }
        Err(err) if allow_error => Some(NudgeReplyMsg {
            exit_code: err.exit_code(),
            pid: last_reply.clone_any().map(|msg| msg.pid).unwrap_or(0),
            error: Some(err.to_string()),
            output: None,
            idle_cache: Some(true),
        }),
        Err(_) => None,
    }
}

fn protocol_identity_holds(q: &QueuedCycle, repo: &std::path::Path) -> bool {
    if !q.runner.is_empty() && q.runner != crate::test_runner::target_request::runner_identity(repo)
    {
        return false;
    }
    if !q.configuration.is_empty()
        && q.configuration != crate::test_runner::target_request::configuration_generation(repo)
    {
        return false;
    }
    true
}

#[cfg(unix)]
pub(super) fn msg_is_workspace(msg: &super::control::NudgeRequestMsg) -> bool {
    msg.is_workspace_focus()
}

#[cfg(unix)]
fn pin_from_nudge_msg(
    msg: &super::control::NudgeRequestMsg,
) -> crate::test_runner::target_request::TargetRequest {
    msg.target_request.clone()
}

#[cfg(unix)]
fn targets_from_pin(
    pin: &crate::test_runner::target_request::TargetRequest,
    _msg: &super::control::NudgeRequestMsg,
) -> Vec<String> {
    crate::test_runner::target_request::operand_raws(&pin.focus).unwrap_or_default()
}

pub(super) fn queued_target_request(
    q: &QueuedCycle,
) -> crate::test_runner::target_request::TargetRequest {
    q.target_request.clone()
}

pub(crate) fn idle_cached_reply(mut last: NudgeReplyMsg) -> NudgeReplyMsg {
    let recap = last.output.as_deref().is_some_and(|s| !s.is_empty());
    let keep_cov_gate = last
        .error
        .as_deref()
        .is_some_and(|e| e.contains("coverage gate failed"));
    if recap && !keep_cov_gate {
        last.error = None;
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
    _queued: &Option<QueuedCycle>,
    machine: &mut SettleMachine,
    repo_root: &Path,
) {
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
