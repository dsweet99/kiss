#![cfg(unix)]

use std::fs;
use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, init_git_repo};

fn write_rust_sigint_repo(dir: &Path) {
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"fake_test_sigint\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/lib.rs"),
        r#"
#[test]
fn test_fast() {
    assert_eq!(1 + 1, 2);
}

#[test]
fn test_slow() {
    std::thread::sleep(std::time::Duration::from_secs(3));
}
"#,
    )
    .unwrap();
}

#[test]
fn kiss_test_sigint_caches_passed_tests_as_it_goes() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_rust_sigint_repo(tmp.path());
    commit_all(tmp.path(), "init");

    let mut child = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "."])
        .current_dir(tmp.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn kiss test .");

    let mut stdout_pipe = child.stdout.take().expect("stdout");
    let collected = Arc::new(Mutex::new(String::new()));
    let collected_reader = Arc::clone(&collected);
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdout_pipe.read(&mut buf) {
            if n == 0 {
                break;
            }
            collected_reader
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push_str(&String::from_utf8_lossy(&buf[..n]));
        }
    });

    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let snap = collected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if snap.contains("PASS: src/lib.rs::test_fast") {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            panic!("timed out waiting for test_fast to pass; stdout={snap:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let pid = child.id() as i32;
    unsafe {
        assert_eq!(libc::kill(pid, libc::SIGINT), 0);
    }
    let status = child.wait().expect("wait");
    let _ = reader.join();
    assert!(
        status.code() == Some(130) || status.signal() == Some(libc::SIGINT),
        "expected interrupted exit 130 or SIGINT, got {status:?}"
    );

    let second_out = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .args(["test", "."])
        .current_dir(tmp.path())
        .output()
        .expect("second kiss test .");

    let stdout2 = String::from_utf8_lossy(&second_out.stdout);
    let stderr2 = String::from_utf8_lossy(&second_out.stderr);
    assert!(
        stdout2.contains("PASS (cached): test_fast"),
        "expected test_fast to be cached on second run, stdout={stdout2}, stderr={stderr2}"
    );
}
