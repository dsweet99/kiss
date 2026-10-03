use std::time::{Duration, Instant};

use super::*;

fn map_oneshot_peer(
    peer: Result<crate::test_runner::OneshotPeer, String>,
) -> Result<crate::test_runner::WatchLockGuard, i32> {
    match peer {
        Ok(crate::test_runner::OneshotPeer::Lock(guard)) => Ok(guard),
        Ok(crate::test_runner::OneshotPeer::WatcherExit(code)) => Err(code),
        Err(e) => {
            eprintln!("error: kiss test: {e}");
            Err(1)
        }
    }
}

fn note_oneshot_wait(next: &mut Option<Instant>) {
    let now = Instant::now();
    if next.is_none_or(|at| now >= at) {
        crate::test_runner::emit_test_progress("kiss test: waiting for kiss test");
        *next = Some(now + Duration::from_secs(3));
    }
}

pub(super) fn take_oneshot_lock(
    args: &TestCommandArgs<'_>,
) -> Result<crate::test_runner::WatchLockGuard, i32> {
    if let Some(overridden) = take_client_result_override() {
        match overridden {
            Ok(Some(code)) => return Err(code),
            Ok(None) => {}
            Err(e) => {
                eprintln!("error: kiss test: {e}");
                return Err(1);
            }
        }
    }
    let cwd = std::env::current_dir().map_err(|e| {
        eprintln!("error: kiss test: {e}");
        1
    })?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd).map_err(|e| {
        eprintln!("error: kiss test: {e}");
        1
    })?;
    let mut next_wait = None;
    map_oneshot_peer(crate::test_runner::wait_oneshot_peer(
        &repo_root,
        || try_wait_out_live_watcher(args),
        || note_oneshot_wait(&mut next_wait),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_oneshot_peer_maps_watcher_exit_and_errors() {
        assert!(matches!(
            map_oneshot_peer(Ok(crate::test_runner::OneshotPeer::WatcherExit(11))),
            Err(11)
        ));
        assert!(matches!(map_oneshot_peer(Err("lock".into())), Err(1)));
    }

    #[test]
    fn note_oneshot_wait_prints_then_waits() {
        let mut next = None;
        note_oneshot_wait(&mut next);
        assert!(next.is_some());
        let stamped = next;
        note_oneshot_wait(&mut next);
        assert_eq!(next, stamped);
    }
}
