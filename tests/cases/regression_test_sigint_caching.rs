//! Correctness of post-CTRL-C minimal work reuse (report.md path 3).
#![cfg(unix)]

use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, init_git_repo};

fn write_python_sigint_repo(dir: &Path) {
    std::fs::write(
        dir.join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         [test]\n\
         test_coverage_threshold = 0\n\
         orphan_detection = false\n\
         num_jobs = 1\n\
         [test.max_unit_test_seconds]\n\
         \"*\" = 30\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();
    std::fs::write(dir.join("lib.py"), "VALUE = 1\n").unwrap();
    std::fs::write(
        dir.join("test_lib.py"),
        concat!(
            "import time\n",
            "\n",
            "\n",
            "def test_fast():\n",
            "    assert True\n",
            "\n",
            "\n",
            "def test_slow():\n",
            "    time.sleep(0.003)\n",
            "    assert True\n",
        ),
    )
    .unwrap();
}

fn kiss_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    cmd.env("NO_COLOR", "1").env("PYTHONDONTWRITEBYTECODE", "1");
    cmd
}

#[test]
fn kiss_test_sigint_caches_passed_tests_as_it_goes() {
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_python_sigint_repo(tmp.path());
    commit_all(tmp.path(), "init");

    let mut child = kiss_cmd()
        .args(["test", "--lang", "python", "."])
        .current_dir(tmp.path())
        .env("PYTHONPATH", tmp.path())
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
        if snap.contains("PASS: test_lib.py::test_fast") || snap.contains("PASS: test_fast") {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            panic!("timed out waiting for test_fast to pass; stdout={snap:?}");
        }
        std::thread::sleep(Duration::from_millis(1));
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

    let second_out = kiss_cmd()
        .args(["test", "--lang", "python", "test_lib.py::test_fast"])
        .current_dir(tmp.path())
        .env("PYTHONPATH", tmp.path())
        .arg("--jobs")
        .arg("1")
        .output()
        .expect("second kiss test test_fast");

    let stdout2 = String::from_utf8_lossy(&second_out.stdout);
    let stderr2 = String::from_utf8_lossy(&second_out.stderr);
    assert!(
        second_out.status.success(),
        "restart must exit 0; stdout={stdout2} stderr={stderr2}"
    );
    assert!(
        stdout2.contains("PASS (cached)")
            && (stdout2.contains("test_fast") || stdout2.contains("test_lib.py")),
        "expected test_fast to be cached on second run, stdout={stdout2}, stderr={stderr2}"
    );
}
