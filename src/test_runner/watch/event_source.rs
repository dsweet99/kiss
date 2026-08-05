//! Watch event source abstraction and notify adapter.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::roots::{WatchRegistration, WatchRootKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NormalizedWatchEvent {
    Paths(Vec<PathBuf>),
    Rescan,
    Error(String),
}

#[derive(Debug)]
pub(crate) enum RecvTimeout {
    Timeout,
    Disconnected(String),
}

pub(crate) trait WatchEventSource {
    fn recv_timeout(&mut self, timeout: Duration) -> Result<Vec<NormalizedWatchEvent>, RecvTimeout>;
}

pub(crate) struct NativeWatchEventSource {
    _watcher: RecommendedWatcher,
    rx: Receiver<Result<notify::Event, notify::Error>>,
}

impl NativeWatchEventSource {
    pub(crate) fn register(registrations: &[WatchRegistration]) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            notify::Config::default(),
        )
        .map_err(|e| format_watch_setup_error(&e, Path::new(".")))?;
        for reg in registrations {
            let mode = match reg.kind {
                WatchRootKind::Recursive => RecursiveMode::Recursive,
                WatchRootKind::NonRecursive => RecursiveMode::NonRecursive,
            };
            watcher.watch(&reg.path, mode).map_err(|e| {
                format_watch_setup_error(&e, &reg.path)
            })?;
        }
        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }
}

impl WatchEventSource for NativeWatchEventSource {
    fn recv_timeout(&mut self, timeout: Duration) -> Result<Vec<NormalizedWatchEvent>, RecvTimeout> {
        let first = match self.rx.recv_timeout(timeout) {
            Ok(msg) => msg,
            Err(RecvTimeoutError::Timeout) => return Err(RecvTimeout::Timeout),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(RecvTimeout::Disconnected(
                    "watch event channel disconnected".into(),
                ));
            }
        };
        let mut out = vec![normalize_notify_result(first)];
        while let Ok(msg) = self.rx.try_recv() {
            out.push(normalize_notify_result(msg));
        }
        Ok(out)
    }
}

pub(crate) fn normalize_notify_result(res: Result<notify::Event, notify::Error>) -> NormalizedWatchEvent {
    match res {
        Ok(event) => normalize_notify_event(event),
        Err(err) => NormalizedWatchEvent::Error(err.to_string()),
    }
}

pub(crate) fn normalize_notify_event(event: notify::Event) -> NormalizedWatchEvent {
    if event.need_rescan() {
        return NormalizedWatchEvent::Rescan;
    }
    match event.kind {
        EventKind::Any
        | EventKind::Access(_)
        | EventKind::Create(_)
        | EventKind::Modify(_)
        | EventKind::Remove(_)
        | EventKind::Other => {
            if event.paths.is_empty() {
                NormalizedWatchEvent::Rescan
            } else {
                NormalizedWatchEvent::Paths(event.paths)
            }
        }
    }
}

pub(crate) fn format_watch_setup_error(err: &notify::Error, root: &Path) -> String {
    let base = format!("failed to watch {}: {err}", root.display());
    if matches!(err.kind, notify::ErrorKind::MaxFilesWatch) {
        format!(
            "{base}; on Linux raise fs.inotify.max_user_watches or choose a narrower target"
        )
    } else {
        base
    }
}

/// Fake event source for unit tests.
#[cfg(test)]
pub(crate) struct FakeWatchEventSource {
    pub events: Vec<NormalizedWatchEvent>,
    pub disconnected: Option<String>,
}

#[cfg(test)]
impl WatchEventSource for FakeWatchEventSource {
    fn recv_timeout(&mut self, _timeout: Duration) -> Result<Vec<NormalizedWatchEvent>, RecvTimeout> {
        if let Some(msg) = self.disconnected.take() {
            return Err(RecvTimeout::Disconnected(msg));
        }
        if self.events.is_empty() {
            return Err(RecvTimeout::Timeout);
        }
        Ok(std::mem::take(&mut self.events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::Event;

    #[test]
    fn normalize_paths_event() {
        let event = Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Any,
        )))
        .add_path(PathBuf::from("a.py"));
        match normalize_notify_event(event) {
            NormalizedWatchEvent::Paths(paths) => assert_eq!(paths, vec![PathBuf::from("a.py")]),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn normalize_empty_paths_is_rescan() {
        let event = Event::new(notify::EventKind::Other);
        assert_eq!(normalize_notify_event(event), NormalizedWatchEvent::Rescan);
    }

    #[test]
    fn format_mentions_inotify_hint_for_max_watches() {
        let err = notify::Error::new(notify::ErrorKind::MaxFilesWatch);
        let msg = format_watch_setup_error(&err, Path::new("/tmp/root"));
        assert!(msg.contains("max_user_watches") || msg.contains("failed to watch"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_register_on_tempdir() {
        let tmp = tempfile::tempdir().unwrap();
        let regs = vec![super::super::roots::WatchRegistration {
            path: tmp.path().to_path_buf(),
            kind: super::super::roots::WatchRootKind::NonRecursive,
        }];
        let mut src = NativeWatchEventSource::register(&regs).unwrap();
        std::fs::write(tmp.path().join("x.py"), "1\n").unwrap();
        let _ = src.recv_timeout(Duration::from_secs(2));
    }
}
