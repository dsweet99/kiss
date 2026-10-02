#![cfg(unix)]

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::support::git::{commit_all, init_git_repo};
use crate::support::watch_proc::{WatchProc, start_watch_logged, write_kissconfig_with_threshold};

fn last_passed(text: &str) -> Option<usize> {
    text.lines().rev().find_map(|line| {
        let rest = line
            .strip_prefix("✓ ")
            .or_else(|| line.strip_prefix("✗ "))?;
        rest.split(" passed").next()?.trim().parse().ok()
    })
}

fn oneshot(dir: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    cmd.env("PYTHONDONTWRITEBYTECODE", "1");
    let output = cmd.args(args).current_dir(dir).output().expect("oneshot");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "kiss {args:?} failed; stdout={stdout} stderr={stderr}"
    );
    stdout
}

fn wait_log(log: &Path, watch: &mut WatchProc, pred: impl Fn(&str) -> bool, secs: u64) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if pred(&text) {
            return;
        }
        assert!(watch.still_running(), "watcher died; log={text}");
        assert!(
            Instant::now() < deadline,
            "timed out waiting for watcher; log={text}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn write_bilingual_repo(root: &Path) {
    write_kissconfig_with_threshold(root, 0.01, 0);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("lib.py"), "def f(i):\n    return i\n").unwrap();
    let mut py = String::from("from lib import f\n");
    for i in 0..11 {
        py.push_str(&format!("def test_{i}():\n    assert f({i}) == {i}\n"));
    }
    std::fs::write(root.join("test_lib.py"), py).unwrap();
    let mut rs = String::from("pub fn value(i: u32) -> u32 { i }\n#[cfg(test)]\nmod tests {\n");
    for i in 0..13 {
        rs.push_str(&format!(
            "    #[test]\n    fn test_{i}() {{\n        assert_eq!(super::value({i}), {i});\n    }}\n"
        ));
    }
    rs.push_str("}\n");
    std::fs::write(root.join("src").join("lib.rs"), rs).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    crate::common::generate_lockfile(root);
}

#[test]
fn watch_and_oneshot_report_24_then_11_and_13() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_bilingual_repo(tmp.path());
    commit_all(tmp.path(), "init");

    let log = tmp.path().join("watch.log");
    let mut watch = start_watch_logged(tmp.path(), &["test", "--watch"], &log);
    wait_log(&log, &mut watch, |t| t.contains("kiss test: Waiting"), 90);
    write_kissconfig_with_threshold(tmp.path(), 0.02, 0);
    wait_log(
        &log,
        &mut watch,
        |t| t.matches("✓ ").count() >= 2 && t.matches("kiss test: Waiting").count() >= 2,
        90,
    );

    let watch_text = std::fs::read_to_string(&log).unwrap();
    let watch_n = last_passed(&watch_text).expect("watcher ✓ N passed");
    let all = oneshot(tmp.path(), &["test"]);
    let py = oneshot(tmp.path(), &["test", "--lang", "python"]);
    let rs = oneshot(tmp.path(), &["test", "--lang", "rust"]);
    assert_eq!(watch_n, 24, "watcher log={watch_text}");
    assert_eq!(last_passed(&all), Some(24), "kiss test={all}");
    assert_eq!(last_passed(&py), Some(11), "python={py}");
    assert_eq!(last_passed(&rs), Some(13), "rust={rs}");
}
