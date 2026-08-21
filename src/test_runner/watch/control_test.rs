use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::{Duration, Instant};

use super::*;
use crate::test_runner::watch::lock::{WatchLockGuard, watch_lock_path};

#[test]
fn protocol_round_trip_on_socket() {
    let tmp = tempfile::tempdir().unwrap();
    let sock = tmp.path().join("c.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let req: NudgeRequestMsg = read_framed_json(&mut stream).unwrap();
        assert!(req.force);
        write_framed_json(
            &mut stream,
            &NudgeReplyMsg {
                exit_code: 7,
                pid: 42,
                error: None,
                output: Some("FAIL tests/a.py::t".into()),
            },
        )
        .unwrap();
    });
    let mut client = UnixStream::connect(&sock).unwrap();
    write_framed_json(
        &mut client,
        &NudgeRequestMsg {
            force: true,
            force_bad: false,
            metrics: false,
        },
    )
    .unwrap();
    let reply: NudgeReplyMsg = read_framed_json(&mut client).unwrap();
    assert_eq!(
        reply,
        NudgeReplyMsg {
            exit_code: 7,
            pid: 42,
            error: None,
            output: Some("FAIL tests/a.py::t".into()),
        }
    );
    server.join().unwrap();
}

#[test]
fn stale_session_ignored_when_lock_free() {
    let tmp = tempfile::tempdir().unwrap();
    write_session_file(
        &session_file_path(tmp.path()),
        &SessionFile {
            pid: 1,
            socket: "/tmp/nope.sock".into(),
        },
    )
    .unwrap();
    let result = try_client_nudge(tmp.path(), &NudgeRequestMsg::default()).unwrap();
    assert!(result.is_none(), "free lock means no client path");
}

#[test]
fn lock_held_missing_session_retries_then_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().to_path_buf();
    let lock_path = watch_lock_path(&repo);
    let _lock = WatchLockGuard::lock(&lock_path).unwrap();

    let repo_server = repo.clone();
    let server = thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        let control = WatchControlServer::start(&repo_server).unwrap();
        let req = control
            .nudge_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        let _ = req.reply.send(NudgeReplyMsg {
            exit_code: 0,
            pid: std::process::id(),
            error: None,
            output: None,
        });
        thread::sleep(Duration::from_millis(50));
        drop(control);
    });

    let reply = try_client_nudge(&repo, &NudgeRequestMsg::default())
        .unwrap()
        .expect("should become client");
    assert_eq!(reply.exit_code, 0);
    server.join().unwrap();
}

#[test]
fn second_watch_try_lock_fails_while_held() {
    let tmp = tempfile::tempdir().unwrap();
    let lock_path = watch_lock_path(tmp.path());
    let _first = WatchLockGuard::try_lock(&lock_path).unwrap().unwrap();
    assert!(WatchLockGuard::try_lock(&lock_path).unwrap().is_none());
}

#[test]
fn concurrent_probes_without_watcher_all_see_none() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().to_path_buf();
    let mut handles = Vec::new();
    for _ in 0..8 {
        let repo = repo.clone();
        handles.push(thread::spawn(move || probe_live_watcher(&repo)));
    }
    for handle in handles {
        let seen = handle.join().unwrap().expect("probe must not error");
        assert!(seen.is_none(), "free lock must not invent a watcher");
    }
}

#[test]
fn acquire_waits_out_shared_probe() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().to_path_buf();
    let lock_path = watch_lock_path(&repo);
    let shared = WatchLockGuard::try_lock_shared(&lock_path)
        .unwrap()
        .expect("shared probe");
    let repo_for_watch = repo.clone();
    let handle = thread::spawn(move || WatchSessionOwner::acquire(&repo_for_watch));
    thread::sleep(Duration::from_millis(30));
    drop(shared);
    let owner = handle
        .join()
        .unwrap()
        .expect("watcher must start after oneshot probe releases");
    drop(owner);
}

#[test]
fn second_acquire_fails_while_owner_alive() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let owner = WatchSessionOwner::acquire(repo).expect("first watcher");
    let err = match WatchSessionOwner::acquire(repo) {
        Ok(_) => panic!("second watcher must not acquire while the first is alive"),
        Err(err) => err,
    };
    assert!(
        err.contains("already running"),
        "second watcher must fail; err={err}"
    );
    drop(owner);
}

#[test]
fn probe_times_out_when_lock_held_without_session() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let lock_path = watch_lock_path(repo);
    let _lock = WatchLockGuard::lock(&lock_path).unwrap();
    let t0 = Instant::now();
    let err = probe_live_watcher(repo).expect_err("missing session must time out");
    let elapsed = t0.elapsed();
    assert!(
        err.contains("session is not ready"),
        "err={err}"
    );
    assert!(
        elapsed >= CLIENT_SESSION_RETRY,
        "probe returned too early; elapsed={elapsed:?}"
    );
}

#[test]
fn start_publishes_session_well_before_client_retry() {
    let mut max = Duration::ZERO;
    for _ in 0..20 {
        let tmp = tempfile::tempdir().unwrap();
        let t0 = Instant::now();
        let control = WatchControlServer::start(tmp.path()).unwrap();
        let elapsed = t0.elapsed();
        assert!(
            session_file_path(tmp.path()).is_file(),
            "start must publish session.json"
        );
        drop(control);
        if elapsed > max {
            max = elapsed;
        }
    }
    assert!(
        max < CLIENT_SESSION_RETRY / 2,
        "lock-to-session gap must stay under half the client wait; max={max:?}"
    );
}
