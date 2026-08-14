//! Settle state machine: trailing-edge quiet + metadata age.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathSignature {
    pub exists: bool,
    pub modified: Option<SystemTime>,
    pub length: u64,
}

impl PathSignature {
    pub(crate) fn from_path(path: &Path) -> Self {
        match std::fs::metadata(path) {
            Ok(meta) => Self {
                exists: true,
                modified: meta.modified().ok(),
                length: meta.len(),
            },
            Err(_) => Self {
                exists: false,
                modified: None,
                length: 0,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct PendingPath {
    signature: PathSignature,
    missing_since: Option<Instant>,
}

#[derive(Debug)]
pub(crate) struct SettleMachine {
    settle: Duration,
    pending: BTreeMap<PathBuf, PendingPath>,
    last_event: Option<Instant>,
    deadline: Option<Instant>,
    scope_dirty: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SettlePoll {
    Idle,
    Waiting,
    Ready(Vec<PathBuf>),
    ScopeDirty,
}

impl SettleMachine {
    pub(crate) fn new(settle: Duration) -> Self {
        Self {
            settle,
            pending: BTreeMap::new(),
            last_event: None,
            deadline: None,
            scope_dirty: false,
        }
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(crate) fn note_path(&mut self, path: PathBuf, now: Instant, signature: PathSignature) {
        let missing_since = if signature.exists {
            None
        } else {
            Some(
                self.pending
                    .get(&path)
                    .and_then(|p| p.missing_since)
                    .unwrap_or(now),
            )
        };
        self.pending.insert(
            path,
            PendingPath {
                signature,
                missing_since,
            },
        );
        self.last_event = Some(now);
        self.deadline = Some(now + self.settle);
    }

    pub(crate) fn mark_scope_dirty(&mut self, now: Instant) {
        self.scope_dirty = true;
        self.last_event = Some(now);
        self.deadline = Some(now + self.settle);
    }

    /// Skip the quiet period and return pending paths (or `ScopeDirty`) immediately.
    ///
    /// Idle with an empty pending set is not handled here: callers should run a cycle
    /// directly instead of calling `force_ready`.
    pub(crate) fn force_ready(
        &mut self,
        now: Instant,
        mut refresh: impl FnMut(&Path) -> PathSignature,
    ) -> SettlePoll {
        if self.scope_dirty {
            self.scope_dirty = false;
            self.deadline = None;
            self.last_event = None;
            self.pending.clear();
            return SettlePoll::ScopeDirty;
        }
        if self.pending.is_empty() {
            return SettlePoll::Idle;
        }
        // Refresh once so callers see current signatures, but do not re-arm settle.
        let _ = self.refresh_pending_unsettled(now, &mut refresh);
        let mut paths: Vec<PathBuf> = self.pending.keys().cloned().collect();
        self.pending.clear();
        self.deadline = None;
        self.last_event = None;
        paths.sort();
        SettlePoll::Ready(paths)
    }

    pub(crate) fn poll(
        &mut self,
        now: Instant,
        mut refresh: impl FnMut(&Path) -> PathSignature,
    ) -> SettlePoll {
        if let Some(outcome) = self.poll_scope_dirty(now) {
            return outcome;
        }
        if self.pending.is_empty() {
            return SettlePoll::Idle;
        }
        if !self.deadline_elapsed(now) {
            return SettlePoll::Waiting;
        }
        if self.refresh_pending_unsettled(now, &mut refresh) {
            self.deadline = Some(now + self.settle);
            return SettlePoll::Waiting;
        }
        let mut paths: Vec<PathBuf> = self.pending.keys().cloned().collect();
        self.pending.clear();
        self.deadline = None;
        self.last_event = None;
        paths.sort();
        SettlePoll::Ready(paths)
    }

    fn poll_scope_dirty(&mut self, now: Instant) -> Option<SettlePoll> {
        if !self.scope_dirty {
            return None;
        }
        if self.deadline_elapsed(now) {
            self.scope_dirty = false;
            self.deadline = None;
            self.last_event = None;
            self.pending.clear();
            Some(SettlePoll::ScopeDirty)
        } else {
            Some(SettlePoll::Waiting)
        }
    }

    fn deadline_elapsed(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    fn refresh_pending_unsettled(
        &mut self,
        now: Instant,
        refresh: &mut impl FnMut(&Path) -> PathSignature,
    ) -> bool {
        let paths: Vec<PathBuf> = self.pending.keys().cloned().collect();
        let mut unsettled = false;
        for path in paths {
            if self.update_one_pending(&path, now, refresh) {
                unsettled = true;
            }
        }
        unsettled
    }

    fn update_one_pending(
        &mut self,
        path: &Path,
        now: Instant,
        refresh: &mut impl FnMut(&Path) -> PathSignature,
    ) -> bool {
        let fresh = refresh(path);
        let entry = self.pending.get_mut(path).expect("pending path");
        if fresh != entry.signature {
            entry.missing_since = if fresh.exists {
                None
            } else {
                Some(entry.missing_since.unwrap_or(now))
            };
            entry.signature = fresh;
            return true;
        }
        !path_age_settled(entry, now, self.settle)
    }
}

fn path_age_settled(entry: &PendingPath, now: Instant, settle: Duration) -> bool {
    if entry.signature.exists {
        entry
            .signature
            .modified
            .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
            .is_some_and(|age| age >= settle)
    } else {
        entry
            .missing_since
            .is_some_and(|since| now.duration_since(since) >= settle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(exists: bool, age: Duration) -> PathSignature {
        PathSignature {
            exists,
            modified: if exists {
                Some(SystemTime::now() - age)
            } else {
                None
            },
            length: if exists { 10 } else { 0 },
        }
    }

    #[test]
    fn single_edit_fires_after_settle() {
        let settle = Duration::from_millis(100);
        let mut m = SettleMachine::new(settle);
        let t0 = Instant::now();
        let settled_sig = sig(true, settle);
        m.note_path(PathBuf::from("a.py"), t0, settled_sig.clone());
        assert_eq!(
            m.poll(t0 + Duration::from_millis(50), |_| settled_sig.clone()),
            SettlePoll::Waiting
        );
        let ready = m.poll(t0 + settle, |_| settled_sig.clone());
        assert!(matches!(ready, SettlePoll::Ready(paths) if paths == [PathBuf::from("a.py")]));
    }

    #[test]
    fn repeated_writes_reset_deadline() {
        let settle = Duration::from_millis(100);
        let mut m = SettleMachine::new(settle);
        let t0 = Instant::now();
        let recent = sig(true, Duration::from_millis(20));
        m.note_path(PathBuf::from("a.py"), t0, recent.clone());
        m.note_path(
            PathBuf::from("a.py"),
            t0 + Duration::from_millis(80),
            recent.clone(),
        );
        assert_eq!(
            m.poll(t0 + Duration::from_millis(100), |_| recent.clone()),
            SettlePoll::Waiting
        );
    }

    #[test]
    fn too_recent_mtime_postpones() {
        let settle = Duration::from_secs(1);
        let mut m = SettleMachine::new(settle);
        let t0 = Instant::now();
        let recent = sig(true, Duration::from_millis(10));
        m.note_path(PathBuf::from("a.py"), t0, recent.clone());
        assert_eq!(
            m.poll(t0 + settle, |_| recent.clone()),
            SettlePoll::Waiting
        );
    }

    #[test]
    fn delete_settles_after_missing_age() {
        let settle = Duration::from_millis(50);
        let mut m = SettleMachine::new(settle);
        let t0 = Instant::now();
        let missing = sig(false, Duration::ZERO);
        m.note_path(PathBuf::from("gone.py"), t0, missing.clone());
        let ready = m.poll(t0 + settle, |_| missing.clone());
        assert!(matches!(ready, SettlePoll::Ready(_)));
    }

    #[test]
    fn scope_dirty_after_settle() {
        let settle = Duration::from_millis(20);
        let mut m = SettleMachine::new(settle);
        let t0 = Instant::now();
        m.mark_scope_dirty(t0);
        let any = sig(true, settle);
        assert_eq!(m.poll(t0, |_| any.clone()), SettlePoll::Waiting);
        assert_eq!(
            m.poll(t0 + settle, |_| any.clone()),
            SettlePoll::ScopeDirty
        );
    }

    #[test]
    fn force_ready_returns_pending_before_settle() {
        let settle = Duration::from_secs(30);
        let mut m = SettleMachine::new(settle);
        let t0 = Instant::now();
        let settled_sig = sig(true, Duration::from_secs(60));
        m.note_path(PathBuf::from("a.py"), t0, settled_sig.clone());
        assert_eq!(
            m.poll(t0 + Duration::from_millis(1), |_| settled_sig.clone()),
            SettlePoll::Waiting
        );
        let ready = m.force_ready(t0 + Duration::from_millis(1), |_| settled_sig.clone());
        assert!(matches!(ready, SettlePoll::Ready(paths) if paths == [PathBuf::from("a.py")]));
        assert_eq!(
            m.poll(t0 + Duration::from_millis(2), |_| settled_sig.clone()),
            SettlePoll::Idle
        );
    }
}
