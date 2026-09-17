use super::*;
use crate::test_runner::test_mode_fixtures::{git_in, init_git, python_dry_run_args};
use crate::test_runner::watch::event_source::{
    FakeWatchEventSource, NormalizedWatchEvent, RecvTimeout, WatchEventSource,
};
use std::collections::VecDeque;
use std::env;
use std::time::Duration;

struct SettleScript {
    steps: VecDeque<Result<Vec<NormalizedWatchEvent>, RecvTimeout>>,
}

impl WatchEventSource for SettleScript {
    fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Vec<NormalizedWatchEvent>, RecvTimeout> {
        match self.steps.pop_front() {
            Some(Err(RecvTimeout::Timeout)) => {
                std::thread::sleep(timeout.min(Duration::from_millis(15)));
                Err(RecvTimeout::Timeout)
            }
            Some(other) => other,
            None => Err(RecvTimeout::Disconnected("settle-script-done".into())),
        }
    }
}

#[test]
fn settle_cycle_then_disconnect() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    let file = tmp.path().join("a.py");
    std::fs::write(&file, "x=1\n").unwrap();
    let _ = std::process::Command::new("touch")
        .args(["-d", "1970-01-01 00:00:01", file.to_str().unwrap()])
        .status();
    assert!(
        git_in(tmp.path())
            .args(["add", "a.py"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    let orig = env::current_dir().unwrap();
    env::set_current_dir(tmp.path()).unwrap();
    let mut steps = VecDeque::new();
    steps.push_back(Err(RecvTimeout::Timeout));
    steps.push_back(Ok(vec![NormalizedWatchEvent::Paths(vec![file.clone()])]));
    steps.push_back(Err(RecvTimeout::Timeout));
    steps.push_back(Err(RecvTimeout::Disconnected("done".into())));
    let mut src = SettleScript { steps };
    let args = python_dry_run_args(vec!["a.py".into()]);
    let code = run_watch_loop(args, Duration::from_millis(5), tmp.path(), &mut src, None);
    env::set_current_dir(orig).unwrap();
    assert_eq!(code, 1);
}

#[cfg(unix)]
#[test]
fn watch_cycle_logs_starting() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    std::fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "a.py"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    let orig = env::current_dir().unwrap();
    env::set_current_dir(tmp.path()).unwrap();
    let mut src = FakeWatchEventSource {
        events: vec![],
        disconnected: Some("boom".into()),
    };
    let args = python_dry_run_args(vec!["a.py".into()]);
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let _ = run_watch_loop(args, Duration::from_millis(5), tmp.path(), &mut src, None);
    });
    env::set_current_dir(orig).unwrap();
    assert!(
        out.contains("kiss test: Starting"),
        "watcher must log when it starts a test cycle; stdout={out:?}"
    );
}

#[test]
fn fake_disconnect_after_first_cycle() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    std::fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "a.py"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    let orig = env::current_dir().unwrap();
    env::set_current_dir(tmp.path()).unwrap();
    let mut src = FakeWatchEventSource {
        events: vec![],
        disconnected: Some("boom".into()),
    };
    let args = python_dry_run_args(vec!["a.py".into()]);
    let code = run_watch_loop(args, Duration::from_millis(5), tmp.path(), &mut src, None);
    env::set_current_dir(orig).unwrap();
    assert_eq!(code, 1);
}
