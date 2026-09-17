use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::bin_cli::args::TestInvocation;

use super::filter::{config_rel_for_watch, path_should_enter_watch_queue};
use super::roots::{WatchRegistration, WatchRootKind};

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
pub(crate) static TEST_IMMEDIATE_DISCONNECT: AtomicBool = AtomicBool::new(false);

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
    fn recv_timeout(&mut self, timeout: Duration)
    -> Result<Vec<NormalizedWatchEvent>, RecvTimeout>;
}

pub(crate) struct NativeWatchEventSource {
    _watcher: RecommendedWatcher,
    rx: Receiver<Result<notify::Event, notify::Error>>,
}

impl NativeWatchEventSource {
    pub(crate) fn register(
        registrations: &[WatchRegistration],
        repo_root: &Path,
        invocation: &TestInvocation,
        config_path: &Path,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let repo_root = repo_root.to_path_buf();
        let invocation = invocation.clone();
        let watched_config = config_rel_for_watch(&repo_root, config_path);
        let mut watcher = RecommendedWatcher::new(
            move |res| {
                if event_should_enter_watch_queue(&res, &repo_root, &invocation, &watched_config) {
                    let _ = tx.send(res);
                }
            },
            notify::Config::default(),
        )
        .map_err(|e| format_watch_setup_error(&e, Path::new(".")))?;
        for reg in registrations {
            let mode = match reg.kind {
                WatchRootKind::Recursive => RecursiveMode::Recursive,
                WatchRootKind::NonRecursive => RecursiveMode::NonRecursive,
            };
            watcher
                .watch(&reg.path, mode)
                .map_err(|e| format_watch_setup_error(&e, &reg.path))?;
        }
        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }
}

impl WatchEventSource for NativeWatchEventSource {
    fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Vec<NormalizedWatchEvent>, RecvTimeout> {
        #[cfg(test)]
        if TEST_IMMEDIATE_DISCONNECT.load(Ordering::SeqCst) {
            return Err(RecvTimeout::Disconnected(
                "TEST_IMMEDIATE_DISCONNECT".into(),
            ));
        }
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

pub(crate) fn event_should_enter_watch_queue(
    res: &Result<notify::Event, notify::Error>,
    repo_root: &Path,
    invocation: &TestInvocation,
    watched_config: &Path,
) -> bool {
    match res {
        Err(_) => true,
        Ok(event) => {
            notify_event_should_enter_watch_queue(event, repo_root, invocation, watched_config)
        }
    }
}

fn notify_event_should_enter_watch_queue(
    event: &notify::Event,
    repo_root: &Path,
    invocation: &TestInvocation,
    watched_config: &Path,
) -> bool {
    if event.need_rescan() {
        return true;
    }
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    if event.paths.is_empty() {
        return true;
    }
    event
        .paths
        .iter()
        .any(|path| queueable_watch_path(path, repo_root, invocation, watched_config))
}

fn queueable_watch_path(
    path: &Path,
    repo_root: &Path,
    invocation: &TestInvocation,
    watched_config: &Path,
) -> bool {
    let rel = path.strip_prefix(repo_root).unwrap_or(path);
    path_should_enter_watch_queue(rel, invocation, watched_config)
}

pub(crate) fn normalize_notify_result(
    res: Result<notify::Event, notify::Error>,
) -> NormalizedWatchEvent {
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
        format!("{base}; on Linux raise fs.inotify.max_user_watches or choose a narrower target")
    } else {
        base
    }
}

#[cfg(test)]
pub(crate) struct FakeWatchEventSource {
    pub events: Vec<NormalizedWatchEvent>,
    pub disconnected: Option<String>,
}

#[cfg(test)]
impl WatchEventSource for FakeWatchEventSource {
    fn recv_timeout(
        &mut self,
        _timeout: Duration,
    ) -> Result<Vec<NormalizedWatchEvent>, RecvTimeout> {
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
    use crate::bin_cli::args::TestInvocation;
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

    #[test]
    fn access_events_do_not_enter_the_watch_queue() {
        let event = Event::new(notify::EventKind::Access(notify::event::AccessKind::Any))
            .add_path(PathBuf::from("/repo/app.py"));
        assert!(!event_should_enter_watch_queue(
            &Ok(event),
            Path::new("/repo"),
            &TestInvocation::All,
            Path::new(".kissconfig"),
        ));
    }

    #[test]
    fn kiss_cache_writes_do_not_enter_the_watch_queue() {
        let event = Event::new(notify::EventKind::Create(notify::event::CreateKind::File))
            .add_path(PathBuf::from("/repo/.kiss/junk/a.txt"));
        assert!(!event_should_enter_watch_queue(
            &Ok(event),
            Path::new("/repo"),
            &TestInvocation::All,
            Path::new(".kissconfig"),
        ));
    }

    #[test]
    fn source_modifies_do_enter_the_watch_queue() {
        let event = Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Any,
        )))
        .add_path(PathBuf::from("/repo/app.py"));
        assert!(event_should_enter_watch_queue(
            &Ok(event),
            Path::new("/repo"),
            &TestInvocation::All,
            Path::new(".kissconfig"),
        ));
    }

    #[test]
    fn git_exclude_support_path_still_enters_the_watch_queue() {
        let event = Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Any,
        )))
        .add_path(PathBuf::from("/repo/.git/info/exclude"));
        assert!(event_should_enter_watch_queue(
            &Ok(event),
            Path::new("/repo"),
            &TestInvocation::All,
            Path::new(".kissconfig"),
        ));
    }

    #[test]
    fn in_tree_non_source_files_do_not_enter_the_watch_queue() {
        let event = Event::new(notify::EventKind::Create(notify::event::CreateKind::File))
            .add_path(PathBuf::from("/repo/noise.txt"));
        assert!(!event_should_enter_watch_queue(
            &Ok(event),
            Path::new("/repo"),
            &TestInvocation::All,
            Path::new(".kissconfig"),
        ));
    }

    #[test]
    fn watched_config_override_enters_the_watch_queue() {
        let event = Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Any,
        )))
        .add_path(PathBuf::from("/repo/custom.toml"));
        assert!(event_should_enter_watch_queue(
            &Ok(event),
            Path::new("/repo"),
            &TestInvocation::All,
            Path::new("custom.toml"),
        ));
    }

    #[test]
    fn watched_parent_relative_config_outside_repo_enters_the_watch_queue() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        let config = tmp.path().join("watch.toml");
        std::fs::write(&config, "[test]\n").unwrap();
        let event = Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Any,
        )))
        .add_path(config);
        let watched_config = config_rel_for_watch(&repo, Path::new("../watch.toml"));
        assert!(event_should_enter_watch_queue(
            &Ok(event),
            &repo,
            &TestInvocation::All,
            &watched_config,
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_register_on_tempdir() {
        let tmp = tempfile::tempdir().unwrap();
        let regs = vec![super::super::roots::WatchRegistration {
            path: tmp.path().to_path_buf(),
            kind: super::super::roots::WatchRootKind::NonRecursive,
        }];
        let mut src = NativeWatchEventSource::register(
            &regs,
            tmp.path(),
            &TestInvocation::All,
            Path::new(".kissconfig"),
        )
        .unwrap();
        std::fs::write(tmp.path().join("x.py"), "1\n").unwrap();
        let _ = src.recv_timeout(Duration::from_secs(2));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_watch_does_not_queue_hard_excluded_cache_writes() {
        use super::super::roots::{WatchRegistration, WatchRootKind};
        let tmp = tempfile::tempdir().unwrap();
        let junk = tmp.path().join(".kiss").join("junk");
        std::fs::create_dir_all(&junk).unwrap();
        std::fs::write(tmp.path().join("app.py"), "x=1\n").unwrap();
        let regs = vec![WatchRegistration {
            path: tmp.path().to_path_buf(),
            kind: WatchRootKind::Recursive,
        }];
        let mut src = NativeWatchEventSource::register(
            &regs,
            tmp.path(),
            &TestInvocation::All,
            Path::new(".kissconfig"),
        )
        .unwrap();
        let _ = src.recv_timeout(Duration::from_millis(150));

        for i in 0..40 {
            std::fs::write(junk.join(format!("f{i}.txt")), b"x").unwrap();
        }
        for i in 0..40 {
            std::fs::write(tmp.path().join(format!("noise{i}.txt")), b"x").unwrap();
        }
        std::fs::write(tmp.path().join("app.py"), "x=2\n").unwrap();

        let mut kiss_hits = 0usize;
        let mut txt_hits = 0usize;
        let mut py_hits = 0usize;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            match src.recv_timeout(Duration::from_millis(200)) {
                Ok(events) => {
                    for event in events {
                        if let NormalizedWatchEvent::Paths(paths) = event {
                            for path in paths {
                                if path.components().any(|c| c.as_os_str() == ".kiss") {
                                    kiss_hits += 1;
                                }
                                if path.extension().is_some_and(|ext| ext == "txt")
                                    && !path.components().any(|c| c.as_os_str() == ".kiss")
                                {
                                    txt_hits += 1;
                                }
                                if path.extension().is_some_and(|ext| ext == "py") {
                                    py_hits += 1;
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    if py_hits > 0 {
                        break;
                    }
                }
            }
        }
        assert_eq!(
            kiss_hits, 0,
            "hard-excluded .kiss writes must not enter the watch queue; queued={kiss_hits}"
        );
        assert_eq!(
            txt_hits, 0,
            "in-tree non-source writes must not enter the watch queue; queued={txt_hits}"
        );
        assert!(
            py_hits > 0,
            "source file changes must still enter the watch queue"
        );
    }
}
