use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const CONFIG: &str = "[global]\n\
duplication_enabled = false\n\
\n\
[test]\n\
[python]\n\
[rust]\n\
\n\
[test.max_unit_test_seconds]\n\
\"tests/slow/dbs\" = 180\n\
\"tests/slow\" = 60\n\
\"tests/fast\" = 2\n\
\"tests/\" = 10\n\
\"rust\" = 10\n\
\"*\" = 60\n";

fn write_fixture(repo: &Path) {
    for dir in ["tests/slow/dbs", "tests/slow", "tests/fast", "tests/web"] {
        fs::create_dir_all(repo.join(dir)).unwrap();
    }
    fs::write(repo.join("app.py"), "VALUE = 1\n").unwrap();
    fs::write(repo.join(".kissconfig"), CONFIG).unwrap();
    for (file, name) in [
        ("tests/slow/dbs/test_q.py", "test_q"),
        ("tests/slow/test_other.py", "test_other"),
        ("tests/fast/test_a.py", "test_a"),
        ("tests/web/test_b.py", "test_b"),
        ("src_app_test.py", "test_src"),
    ] {
        fs::write(
            repo.join(file),
            format!("from app import VALUE\n\ndef {name}():\n    assert VALUE == 1\n"),
        )
        .unwrap();
    }
}

fn warm_python_records(repo: &Path, home: &Path) {
    fs::write(repo.join(".kissconfig"), CONFIG).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_build_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    let output = cmd
        .args(["test", "--lang", "python", "."])
        .current_dir(repo)
        .env("HOME", home)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("kiss test should run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("5 passed"),
        "warm-up should run all five tests\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_stats(repo: &Path, home: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("stats")
        .arg(repo)
        .current_dir(repo)
        .env("HOME", home)
        .output()
        .expect("kiss stats should run");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "kiss stats failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stdout
}

fn runtime_rows(stdout: &str) -> Vec<&str> {
    let heading = stdout
        .lines()
        .find(|l| l.starts_with("unit_test_runtime_sec:"))
        .unwrap_or_else(|| panic!("missing unit_test_runtime_sec heading:\n{stdout}"));
    assert!(
        heading.contains("culled from cached values only"),
        "heading missing cache disclaimer: {heading}"
    );
    stdout
        .lines()
        .skip_while(|l| !l.starts_with("unit_test_runtime_sec:"))
        .skip(2)
        .take_while(|l| l.split_whitespace().count() == 8)
        .collect()
}

fn assert_grouped_rows(rows: &[&str]) {
    assert_eq!(
        rows.len(),
        6,
        "expected one row per configured rule, got:\n{}",
        rows.join("\n")
    );
    let expected = [
        ["tests/slow/dbs", "180", "1"],
        ["tests/slow", "60", "1"],
        ["tests/fast", "2", "1"],
        ["tests/", "10", "1"],
        ["rust", "10", "0"],
        ["*", "60", "1"],
    ];
    for (row, expected_cells) in rows.iter().zip(expected) {
        let cells: Vec<&str> = row.split_whitespace().collect();
        assert_eq!(
            &cells[..3],
            expected_cells,
            "row `{row}` did not begin with the expected cells"
        );
    }
    let n_values: Vec<usize> = rows
        .iter()
        .map(|row| {
            row.split_whitespace()
                .nth(2)
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| panic!("bad N cell in {row}"))
        })
        .collect();
    assert_eq!(n_values, vec![1, 1, 1, 1, 0, 1]);
}

#[test]
fn cli_stats_groups_unit_test_runtime_by_configured_test_sets() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    crate::support::git::init_git_repo(repo.path());
    write_fixture(repo.path());
    crate::support::git::commit_all(repo.path(), "init");
    warm_python_records(repo.path(), home.path());
    let stdout = run_stats(repo.path(), home.path());
    assert_grouped_rows(&runtime_rows(&stdout));
}
