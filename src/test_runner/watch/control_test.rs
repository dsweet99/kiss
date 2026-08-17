use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::Duration;

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
        let req = control.nudge_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let _ = req.reply.send(NudgeReplyMsg {
            exit_code: 0,
            pid: std::process::id(),
            error: None,
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
