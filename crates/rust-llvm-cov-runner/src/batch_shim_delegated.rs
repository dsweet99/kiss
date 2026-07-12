use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::batch_process_tree::ProcessGroupIdentity;

pub(crate) const DELEGATED_GO_ENV: &str = "KISS_RUST_LLVM_COV_DELEGATED_GO";

pub(crate) fn spawn_delegated_piped_child(
    delegated: &[String],
    command: &[OsString],
    profile_path: &Path,
    go_path: &Path,
) -> io::Result<(std::process::Child, Option<ProcessGroupIdentity>)> {
    let mut command = build_handshake_wrapped_command(delegated, command, go_path);
    command.env("LLVM_PROFILE_FILE", profile_path);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.stdin(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn()?;
    let delegated_identity = record_spawned_child_identity(&child);
    Ok((child, delegated_identity))
}

pub(crate) fn release_delegated_child_handshake(go_path: &Path) -> io::Result<()> {
    fs::write(go_path, b"go\n")
}

fn build_handshake_wrapped_command(
    delegated: &[String],
    command: &[OsString],
    go_path: &Path,
) -> Command {
    let mut wrapper = Command::new("/bin/sh");
    wrapper.arg("-c").arg(
        "while [ ! -f \"$KISS_RUST_LLVM_COV_DELEGATED_GO\" ]; do sleep 0.001; done; cmd=$1; shift; exec \"$cmd\" \"$@\"",
    );
    wrapper.arg("sh");
    if delegated.is_empty() {
        wrapper.arg(&command[0]);
        wrapper.args(&command[1..]);
    } else {
        wrapper.arg(&delegated[0]);
        wrapper.args(&delegated[1..]);
        wrapper.args(command);
    }
    wrapper.env(DELEGATED_GO_ENV, go_path);
    wrapper
}

fn record_spawned_child_identity(child: &std::process::Child) -> Option<ProcessGroupIdentity> {
    let pid = child.id();
    if pid == 0 {
        return None;
    }
    let pgid = unsafe { libc::getpgid(pid as i32) };
    if pgid <= 0 {
        return None;
    }
    Some(ProcessGroupIdentity {
        pid,
        pgid: pgid as u32,
    })
}
