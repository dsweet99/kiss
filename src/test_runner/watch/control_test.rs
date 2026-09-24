use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::{Duration, Instant};

use super::*;
use crate::test_runner::watch::lock::{WatchLockGuard, watch_lock_path};

fn commit_request() -> crate::test_runner::target_request::TargetRequest {
    use crate::test_runner::target_request::{GitFocus, TargetFocus, request_from_focus};
    request_from_focus(TargetFocus::Git(GitFocus::Commit), None, &[])
}

#[test]
fn nudge_msg_as_request_all_without_pin_is_workspace() {
    use crate::test_runner::target_request::{TargetFocus, is_workspace_focus};
    let all = NudgeRequestMsg::default();
    assert!(matches!(all.as_request().focus, TargetFocus::Workspace));
    assert!(is_workspace_focus(&all.as_request().focus));
    let path = NudgeRequestMsg {
        ..Default::default()
    };
    assert!(matches!(path.as_request().focus, TargetFocus::Workspace));
    let commit = NudgeRequestMsg {
        target_request: commit_request(),
        ..Default::default()
    };
    assert!(matches!(
        commit.as_request().focus,
        crate::test_runner::target_request::TargetFocus::Git(
            crate::test_runner::target_request::GitFocus::Commit
        )
    ));
    assert!(!is_workspace_focus(&commit.as_request().focus));
}

#[test]
fn progress_line_no_request_all_follows_workspace_request() {
    use crate::test_runner::target_request::{is_workspace_focus, workspace_request};
    let all = NudgeRequestMsg::default();
    assert!(all.is_workspace_focus());
    assert!(is_workspace_focus(&workspace_request(None, &[]).focus));
    assert!(!all.progress_line().contains("invocation="));
    let path = NudgeRequestMsg {
        ..Default::default()
    };
    assert!(path.is_workspace_focus());
    assert!(!path.progress_line().contains("invocation="));
    let commit = NudgeRequestMsg {
        target_request: commit_request(),
        ..Default::default()
    };
    assert!(!commit.is_workspace_focus());
    assert!(commit.progress_line().contains("invocation=commit"));
}

#[test]
fn nudge_request_progress_line_includes_fields() {
    assert_eq!(
        NudgeRequestMsg::default().progress_line(),
        "kiss test: request force=false force_bad=false metrics=false"
    );
    assert_eq!(
        NudgeRequestMsg {
            force: true,
            force_bad: true,
            metrics: true,
            target_request: crate::test_runner::target_request::operands_request(
                &["a.rs".into(), "b.py".into()],
                None,
                &[],
            ),
            ..Default::default()
        }
        .progress_line(),
        "kiss test: request force=true force_bad=true metrics=true invocation=targets targets=a.rs b.py"
    );
}

#[test]
fn progress_line_prefers_target_request_over_stale_targets() {
    use crate::test_runner::target_request::operands_request;
    let request = operands_request(&["tests/a.py".into()], None, &[]);
    let line = NudgeRequestMsg {
        target_request: request,
        ..Default::default()
    }
    .progress_line();
    assert!(line.contains("invocation=targets"));
    assert!(line.contains("targets=tests/a.py"));
    assert!(!line.contains("stale.py"));
}

#[test]
fn effective_scope_keeps_operand_request_as_targets() {
    use crate::test_runner::target_request::operands_request;
    let request = operands_request(&["tests/a.py".into()], None, &[]);
    let msg = NudgeRequestMsg {
        target_request: request,
        ..Default::default()
    };
    let (invocation, targets) = msg.effective_scope();
    assert_eq!(invocation, NudgeInvocation::Targets);
    assert!(!invocation.is_all());
    assert_eq!(targets, vec!["tests/a.py".to_string()]);
}

#[test]
fn effective_scope_clones_operand_pin_sorted() {
    use crate::test_runner::target_request::operands_request;
    let request = operands_request(&["z.py".into(), "a.py".into()], None, &[]);
    let msg = NudgeRequestMsg {
        target_request: request,
        ..Default::default()
    };
    let (invocation, targets) = msg.effective_scope();
    assert_eq!(invocation, NudgeInvocation::Targets);
    assert_eq!(targets, vec!["a.py".to_string(), "z.py".to_string()]);
}

#[test]
fn as_request_clones_operand_pin_sorted() {
    use crate::test_runner::target_request::{operand_raws, operands_request};
    let request = operands_request(&["z.py".into(), "a.py".into()], None, &[]);
    let msg = NudgeRequestMsg {
        target_request: request,
        ..Default::default()
    };
    assert_eq!(
        operand_raws(&msg.as_request().focus),
        Some(vec!["a.py".into(), "z.py".into()])
    );
}

#[test]
fn progress_line_prefers_git_request_over_stale_all() {
    use crate::test_runner::target_request::{GitFocus, TargetFocus, request_from_focus};
    let request = request_from_focus(TargetFocus::Git(GitFocus::Commit), None, &[]);
    let line = NudgeRequestMsg {
        target_request: request,
        ..Default::default()
    }
    .progress_line();
    assert!(line.contains("invocation=commit"));
}

#[test]
fn progress_line_sorts_operand_request() {
    use crate::test_runner::target_request::operands_request;
    let request = operands_request(&["z.py".into(), "a.py".into()], None, &[]);
    let line = NudgeRequestMsg {
        target_request: request,
        ..Default::default()
    }
    .progress_line();
    assert!(line.contains("invocation=targets"));
    assert!(line.contains("targets=a.py z.py"));
    assert!(!line.contains("stale.py"));
}

#[test]
fn from_focus_operands_is_targets() {
    use crate::test_runner::target_request::operands_request;
    let request = operands_request(&["z.py".into(), "a.py".into()], None, &[]);
    assert_eq!(
        NudgeInvocation::from_focus(&request.focus),
        NudgeInvocation::Targets
    );
    assert!(!NudgeInvocation::from_focus(&request.focus).is_all());
}

#[test]
fn from_test_maps_operand_invocation_to_targets() {
    use crate::bin_cli::args::TestInvocation;
    assert_eq!(
        NudgeInvocation::from_test(&TestInvocation::Targets(vec!["tests/a.py".into()])),
        NudgeInvocation::Targets
    );
    assert_eq!(
        NudgeInvocation::from_test(&TestInvocation::All),
        NudgeInvocation::All
    );
}

#[test]
fn handle_client_logs_received_request() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().to_path_buf();
    let lock_path = watch_lock_path(&repo);
    let _lock = WatchLockGuard::lock(&lock_path).unwrap();
    let control = WatchControlServer::start(&repo).unwrap();
    let server = thread::spawn(move || {
        let req = control
            .nudge_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        let _ = req.reply.send(NudgeReplyMsg {
            exit_code: 0,
            pid: std::process::id(),
            error: None,
            output: None,
            idle_cache: None,
        });
        drop(control);
    });
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let reply = try_client_nudge(
            &repo,
            &NudgeRequestMsg {
                force: false,
                force_bad: true,
                metrics: true,
                target_request: crate::test_runner::target_request::operands_request(
                    &["src/lib.rs".into()],
                    None,
                    &[],
                ),
                ..Default::default()
            },
        )
        .unwrap()
        .expect("client");
        assert_eq!(reply.exit_code, 0);
    });
    server.join().unwrap();
    assert!(
        out.contains(
            "kiss test: request force=false force_bad=true metrics=true invocation=targets targets=src/lib.rs"
        ),
        "watcher must log the received kiss test request; stdout={out:?}"
    );
}

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
                idle_cache: None,
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
            ..Default::default()
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
            idle_cache: None,
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
fn stale_session_file_removed_when_lock_free() {
    let tmp = tempfile::tempdir().unwrap();
    let session_path = session_file_path(tmp.path());
    write_session_file(
        &session_path,
        &SessionFile {
            pid: 4_294_967_294,
            socket: "/tmp/kiss-watch-stale-nope.sock".into(),
        },
    )
    .unwrap();
    let result = try_client_nudge(tmp.path(), &NudgeRequestMsg::default()).unwrap();
    assert!(result.is_none(), "dead watcher must not be a client path");
    assert!(
        !session_path.is_file(),
        "next kiss test must reclaim stale session.json"
    );
}

#[test]
fn dead_session_pid_ignored_while_lock_held() {
    let tmp = tempfile::tempdir().unwrap();
    let lock_path = watch_lock_path(tmp.path());
    let _lock = WatchLockGuard::lock(&lock_path).unwrap();
    let session_path = session_file_path(tmp.path());
    write_session_file(
        &session_path,
        &SessionFile {
            pid: 4_294_967_294,
            socket: "/tmp/kiss-watch-dead-held.sock".into(),
        },
    )
    .unwrap();
    let result = try_client_nudge(tmp.path(), &NudgeRequestMsg::default()).unwrap();
    assert!(result.is_none(), "dead pid must not be nudged");
    assert!(
        !session_path.is_file(),
        "dead session.json must be reclaimed while the lock is leftover"
    );
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
            idle_cache: None,
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
    assert!(err.contains("session is not ready"), "err={err}");
    assert!(
        elapsed >= CLIENT_SESSION_RETRY,
        "probe returned too early; elapsed={elapsed:?}"
    );
}

#[test]
fn start_publishes_session_well_before_client_retry() {
    let mut samples = Vec::with_capacity(20);
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
        samples.push(elapsed);
    }
    samples.sort();
    let median = samples[samples.len() / 2];
    let max = *samples.last().unwrap();
    assert!(
        median < CLIENT_SESSION_RETRY / 2,
        "typical lock-to-session gap must stay under half the client wait; median={median:?}"
    );
    assert!(
        max < CLIENT_SESSION_RETRY,
        "even the slowest start must beat the full client wait; max={max:?}"
    );
}

#[test]
fn framed_json_accepts_full_workspace_report_size() {
    let payload = "x".repeat(512 * 1024);
    let expected_len = payload.len();
    let (mut reader, mut writer) = UnixStream::pair().unwrap();
    let send = thread::spawn(move || write_framed_json(&mut writer, &payload));
    let received: String = read_framed_json(&mut reader).unwrap();
    send.join().unwrap().unwrap();
    assert_eq!(received.len(), expected_len);
}

#[test]
fn handle_client_empty_hangup_is_not_an_error() {
    let (client, server) = UnixStream::pair().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    drop(client);
    let result = handle_client(server, tx);
    assert_eq!(
        result,
        Ok(()),
        "reclaim-style hangup must not be a control client error"
    );
    assert!(
        rx.try_recv().is_err(),
        "empty hangup must not enqueue a nudge"
    );
}

#[test]
fn reclaim_stale_watch_sockets_removes_dead_socks_keeps_live() {
    let _ = std::fs::create_dir_all(WATCH_SOCKET_TMP_DIR);
    let dead = std::path::PathBuf::from(format!(
        "{WATCH_SOCKET_TMP_DIR}/reclaim-dead-{}.sock",
        std::process::id()
    ));
    let live = std::path::PathBuf::from(format!(
        "{WATCH_SOCKET_TMP_DIR}/reclaim-live-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&dead);
    let _ = std::fs::remove_file(&live);
    {
        let listener = UnixListener::bind(&dead).unwrap();
        drop(listener);
    }
    let live_listener = UnixListener::bind(&live).unwrap();
    reclaim_stale_watch_sockets(None);
    assert!(!dead.exists(), "dead sock must be reclaimed");
    assert!(live.exists(), "live sock must be kept");
    drop(live_listener);
    let _ = std::fs::remove_file(&live);
}
