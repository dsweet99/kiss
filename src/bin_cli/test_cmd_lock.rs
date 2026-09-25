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

fn note_oneshot_wait(printed: &mut bool) {
    if !*printed {
        *printed = true;
        crate::test_runner::emit_test_progress("kiss test: waiting for kiss test");
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
    let mut printed = false;
    map_oneshot_peer(crate::test_runner::wait_oneshot_peer(
        &repo_root,
        || try_wait_out_live_watcher(args),
        || note_oneshot_wait(&mut printed),
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
    fn note_oneshot_wait_prints_once() {
        let mut printed = false;
        note_oneshot_wait(&mut printed);
        assert!(printed);
        note_oneshot_wait(&mut printed);
        assert!(printed);
    }
}
