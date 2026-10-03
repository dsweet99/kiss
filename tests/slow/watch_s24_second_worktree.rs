#![cfg(unix)]

use std::path::Path;
use std::time::{Duration, Instant};

use crate::support::scenario::{Scenario, kiss, skip_under_coverage};
use crate::support::watch_proc::start_watch_logged;

fn read(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

fn wait_idle(log: &Path) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let before = read(log);
        std::thread::sleep(Duration::from_millis(1500));
        let after = read(log);
        if after == before && after.trim_end().ends_with("kiss test: Waiting") {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "second watcher never idle; log={after}"
        );
    }
}

fn requests(log: &Path) -> usize {
    read(log).matches("kiss test: request ").count()
}

#[test]
fn each_worktree_has_its_own_watcher() {
    if skip_under_coverage() {
        return;
    }
    let s = Scenario::new();
    s.python_pass_fail(0.5);
    s.commit();
    let mut first = s.start_watch();
    s.wait_settled();

    let side = tempfile::TempDir::new().unwrap();
    let wt = side.path().join("wt2");
    s.git(&["worktree", "add", "-q", wt.to_str().unwrap()]);
    let log = side.path().join("watch2.log");
    let mut second = start_watch_logged(&wt, &["test-watch"], &log);
    wait_idle(&log);
    assert!(
        second.still_running(),
        "second watcher starts its own session"
    );
    assert!(
        !read(&log).contains("already running"),
        "log={}",
        read(&log)
    );
    s.take_runs();

    let first_requests = s.requests();
    let reply = kiss(&wt, &["test"]);
    assert_eq!(reply.code, Some(1), "{reply:?}");
    assert_eq!(
        reply.summary(),
        "✗ 1 passed · 1 failed · 0 timed out",
        "{reply:?}"
    );
    assert_eq!(requests(&log), 1, "the second worktree's watcher answers");
    assert_eq!(
        s.requests(),
        first_requests,
        "the first watcher does not answer"
    );
    assert!(
        s.take_runs().is_empty(),
        "answered from the second watcher's cache"
    );

    let own = kiss(s.root(), &["test"]);
    assert_eq!(own.code, Some(1), "{own:?}");
    assert_eq!(
        s.requests(),
        first_requests + 1,
        "the first watcher still serves its worktree"
    );
    assert_eq!(
        requests(&log),
        1,
        "the second watcher does not answer the first worktree"
    );

    assert!(first.signal_and_wait(libc::SIGKILL, Duration::from_secs(5)));
    assert!(
        second.still_running(),
        "stopping one watcher does not stop the other"
    );
    let again = kiss(&wt, &["test"]);
    assert_eq!(again.code, Some(1), "{again:?}");
    assert_eq!(requests(&log), 2, "the second watcher still answers");
}
