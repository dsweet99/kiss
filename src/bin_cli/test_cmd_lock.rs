use super::*;

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
    match crate::test_runner::wait_oneshot_peer(
        &repo_root,
        || try_wait_out_live_watcher(args),
        || {
            if !printed {
                printed = true;
                crate::test_runner::emit_test_progress("kiss test: waiting for kiss test");
            }
        },
    ) {
        Ok(crate::test_runner::OneshotPeer::Lock(guard)) => Ok(guard),
        Ok(crate::test_runner::OneshotPeer::WatcherExit(code)) => Err(code),
        Err(e) => {
            eprintln!("error: kiss test: {e}");
            Err(1)
        }
    }
}
