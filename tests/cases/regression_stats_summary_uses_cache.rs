use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn run_stats(home: &std::path::Path, corpus: &std::path::Path) -> std::process::Output {
    kiss_binary()
        .arg("stats")
        .arg("--all")
        .arg(corpus)
        .env("HOME", home)
        .output()
        .unwrap()
}

#[test]
fn regression_stats_summary_replays_warm_output() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let source = repo.path().join("summary.py");
    fs::write(
        &source,
        "def add(a, b):\n    return a + b\n\n\ndef test_add():\n    assert add(1, 2) == 3\n",
    )
    .unwrap();

    let cold = run_stats(home.path(), repo.path());
    let warm = run_stats(home.path(), repo.path());

    assert_eq!(cold.status.code(), warm.status.code());
    assert!(
        {
            let cold_out = String::from_utf8_lossy(&cold.stdout);
            let warm_out = String::from_utf8_lossy(&warm.stdout);
            let mut cold_lines = cold_out
                .lines()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            let mut warm_lines = warm_out
                .lines()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            cold_lines.sort_unstable();
            warm_lines.sort_unstable();
            cold_lines == warm_lines
        },
        "stats cold and warm output should be identical"
    );
}
