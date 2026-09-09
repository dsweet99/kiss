use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use crate::rust_llvm_cov_runner::execute_or_reuse::batch_process_tree::BatchProcessTreeGuard;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_shim::load_live_shim_process_identities;
use crate::rust_llvm_cov_runner::execute_or_reuse::mem_available::check_host_mem_available;

use super::BatchSubprocessRunError;

pub(crate) fn wait_child_with_interruption(
    child: &mut std::process::Child,
    process_tree: &BatchProcessTreeGuard,
    output_dir: &Path,
    seen_shim_metadata: &mut HashSet<String>,
) -> Result<std::process::ExitStatus, BatchSubprocessRunError> {
    loop {
        ingest_live_shim_identities(
            process_tree.registry().as_ref(),
            output_dir,
            seen_shim_metadata,
        );
        if let Err(err) = check_host_mem_available() {
            let _ = process_tree.terminate_descendants(Duration::ZERO);
            let _ = child.kill();
            let _ = child.wait();
            return Err(err.into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                return completed_or_interrupted_status(status, process_tree);
            }
            Ok(None) => {}
            Err(err) => {
                return Err(BatchSubprocessRunError::Spawn {
                    program: "batch".to_string(),
                    message: err.to_string(),
                });
            }
        }
        if process_tree.interrupted() {
            ingest_live_shim_identities(
                process_tree.registry().as_ref(),
                output_dir,
                seen_shim_metadata,
            );
            let _ = process_tree.terminate_descendants(Duration::from_millis(250));
            let _ = child.wait();
            return Err(BatchSubprocessRunError::Interrupted);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn completed_or_interrupted_status(
    status: std::process::ExitStatus,
    process_tree: &BatchProcessTreeGuard,
) -> Result<std::process::ExitStatus, BatchSubprocessRunError> {
    if process_tree.interrupted() && batch_status_was_killed(&status) {
        return Err(BatchSubprocessRunError::Interrupted);
    }
    Ok(status)
}

fn batch_status_was_killed(status: &std::process::ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal().is_some() || status.code() == Some(130)
    }
    #[cfg(not(unix))]
    {
        status.code() == Some(130)
    }
}

pub(crate) fn ingest_live_shim_identities(
    registry: &crate::rust_llvm_cov_runner::execute_or_reuse::batch_process_tree::ProcessTreeRegistry,
    output_dir: &Path,
    seen: &mut HashSet<String>,
) {
    let Ok(identities) = load_live_shim_process_identities(output_dir) else {
        return;
    };
    for identity in identities {
        let key = format!("{}:{}", identity.pid, identity.pgid);
        if seen.insert(key) {
            registry.record(identity);
        }
    }
}
