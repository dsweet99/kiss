use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn write_clone_pair(dir: &std::path::Path) {
    let dup_body = (0..40)
        .map(|i| format!("    a{i} = {i} + {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        dir.join("dup_a.py"),
        format!("def dup_a():\n{dup_body}\n    return a0\n"),
    )
    .unwrap();
    fs::write(
        dir.join("dup_b.py"),
        format!("def dup_b():\n{dup_body}\n    return a0\n"),
    )
    .unwrap();
}

fn parse_duplicate_count(stdout: &str) -> usize {
    let line = stdout
        .lines()
        .find(|l| l.starts_with("Violations:"))
        .unwrap_or_else(|| panic!("missing `Violations:` line in stdout:\n{stdout}"));
    let nums: Vec<usize> = line
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    assert!(
        nums.len() >= 2,
        "expected duplicate and orphan counts in `{line}`\nstdout:\n{stdout}"
    );
    nums[0]
}

#[test]
fn stats_ignores_fake_duplicates_like_check() {
    let tmp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    fs::write(tmp.path().join("app.py"), "def ok():\n    return 1\n").unwrap();
    let fake = tmp.path().join("tests/fake_python");
    fs::create_dir_all(&fake).unwrap();
    write_clone_pair(&fake);
    let config = crate::common::write_builtin_language_config(tmp.path());

    let stats = kiss_binary()
        .current_dir(tmp.path())
        .arg("--config")
        .arg(&config)
        .arg("stats")
        .arg(".")
        .env("HOME", home.path())
        .output()
        .unwrap();
    let stats_stdout = String::from_utf8_lossy(&stats.stdout);
    assert!(
        stats.status.success(),
        "kiss stats should succeed:\n{stats_stdout}"
    );
    assert!(
        stats_stdout.contains("Analyzed: 1 files"),
        "stats must skip tests/fake_python the same way check does:\n{stats_stdout}"
    );
    assert_eq!(
        parse_duplicate_count(&stats_stdout),
        0,
        "fake_ clones must not count as stats duplicate violations:\n{stats_stdout}"
    );

    let check = kiss_binary()
        .current_dir(tmp.path())
        .arg("--config")
        .arg(&config)
        .arg("check")
        .arg(".")
        .env("HOME", home.path())
        .output()
        .unwrap();
    let check_stdout = String::from_utf8_lossy(&check.stdout);
    assert!(
        check.status.success(),
        "kiss check should succeed:\n{check_stdout}"
    );
    assert!(
        check_stdout.contains("Analyzed: 1 files"),
        "check file count must match stats:\n{check_stdout}"
    );
    assert!(
        check_stdout.contains("NO VIOLATIONS"),
        "check must not report ignored fake_ clones:\n{check_stdout}"
    );
}
