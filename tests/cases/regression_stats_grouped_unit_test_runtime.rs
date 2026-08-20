use crate::common::seed_python_runtime_coverage;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const CONFIG: &str = "[global]\n\
duplication_enabled = false\n\
orphan_module_enabled = false\n\
\n\
[test]\n\
test_coverage_threshold = 0\n\
\n\
[test.max_unit_test_seconds]\n\
\"tests/slow/dbs\" = 180\n\
\"tests/slow\" = 60\n\
\"tests/fast\" = 2\n\
\"tests/\" = 10\n\
\"rust\" = 10\n\
\"*\" = 0\n";

fn write_fixture(repo: &Path) {
    for dir in ["tests/slow/dbs", "tests/slow", "tests/fast", "tests/web"] {
        fs::create_dir_all(repo.join(dir)).unwrap();
    }
    fs::write(repo.join("app.py"), "VALUE = 1\n").unwrap();
    fs::write(repo.join(".kissconfig"), CONFIG).unwrap();
    seed_python_runtime_coverage(
        repo,
        &[
            (
                "tests/slow/dbs/test_q.py::test_q",
                vec![("app.py", vec![1])],
            ),
            (
                "tests/slow/test_other.py::test_other",
                vec![("app.py", vec![1])],
            ),
            ("tests/fast/test_a.py::test_a", vec![("app.py", vec![1])]),
            ("tests/web/test_b.py::test_b", vec![("app.py", vec![1])]),
            ("src_app_test.py::test_src", vec![("app.py", vec![1])]),
        ],
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
        heading.contains("coverage cache; may not reflect full test set"),
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
        ["*", "0", "1"],
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
    write_fixture(repo.path());
    let stdout = run_stats(repo.path(), home.path());
    assert_grouped_rows(&runtime_rows(&stdout));
}
