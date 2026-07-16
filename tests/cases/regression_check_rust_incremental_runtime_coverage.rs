use crate::common::generate_lockfile;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn rust_runtime_coverage_refresh_publishes_incremental_generation() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    write_incremental_rust_repo(&repo, "1");
    generate_lockfile(repo.path());

    let cold = run_kiss_check_rust(&home, &repo);
    assert_success("cold kiss check", &cold);

    write_incremental_rust_repo(&repo, "2");
    let incremental = run_kiss_check_rust(&home, &repo);
    assert_success("incremental kiss check", &incremental);
    let incremental_stderr = String::from_utf8_lossy(&incremental.stderr);
    let (reused, rerun) = parse_incremental_refresh_counts(&incremental_stderr)
        .unwrap_or_else(|| panic!("missing incremental Rust refresh line:\n{incremental_stderr}"));
    assert!(
        reused > 0 && rerun > 0,
        "ordinary source edit should use the incremental Rust refresh path.\nstderr:\n{incremental_stderr}"
    );

    let warm = run_kiss_check_rust(&home, &repo);
    assert_success("warm kiss check", &warm);
    let warm_stderr = String::from_utf8_lossy(&warm.stderr);
    assert!(
        !warm_stderr.contains("refreshing Rust runtime coverage"),
        "warm check should validate the merged generation without refreshing.\nstderr:\n{warm_stderr}"
    );
}

fn parse_incremental_refresh_counts(stderr: &str) -> Option<(usize, usize)> {
    let prefix = "kiss check: incrementally refreshing Rust runtime coverage (";
    let line = stderr.lines().find(|line| line.starts_with(prefix))?;
    let counts = line.strip_prefix(prefix)?.strip_suffix(')')?;
    let (reused, rerun) = counts.split_once(" reused, ")?;
    let rerun = rerun.strip_suffix(" rerun")?;
    Some((reused.parse().ok()?, rerun.parse().ok()?))
}

fn write_incremental_rust_repo(repo: &TempDir, value: &str) {
    let covered = repo.path().join("covered");
    let stable = repo.path().join("stable");
    fs::create_dir_all(covered.join("src")).unwrap();
    fs::create_dir_all(covered.join("tests")).unwrap();
    fs::create_dir_all(stable.join("src")).unwrap();
    fs::create_dir_all(stable.join("tests")).unwrap();
    fs::write(
        repo.path().join(".kissconfig"),
        "[gate]\n\
         test_coverage_threshold = 100\n\
         duplication_enabled = false\n\
         orphan_module_enabled = false\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[workspace]\n\
         members = [\"covered\", \"stable\"]\n\
         resolver = \"3\"\n",
    )
    .unwrap();
    write_covered_package(&covered, value);
    write_stable_package(&stable);
}

fn write_covered_package(covered: &std::path::Path, value: &str) {
    fs::write(
        covered.join("Cargo.toml"),
        "[package]\n\
         name = \"covered\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        covered.join("src").join("lib.rs"),
        format!("pub fn value() -> i32 {{ {value} }}\n"),
    )
    .unwrap();
    fs::write(
        covered.join("tests").join("covers_lib.rs"),
        format!(
            "use covered::value;\n\n\
             #[test]\n\
             fn covers_lib() {{\n\
                 assert_eq!(value(), {value});\n\
             }}\n"
        ),
    )
    .unwrap();
}

fn write_stable_package(stable: &std::path::Path) {
    fs::write(
        stable.join("Cargo.toml"),
        "[package]\n\
         name = \"stable\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        stable.join("src").join("lib.rs"),
        "pub fn stable_value() -> i32 { 10 }\n",
    )
    .unwrap();
    fs::write(
        stable.join("tests").join("covers_stable.rs"),
        "use stable::stable_value;\n\n\
         #[test]\n\
         fn covers_stable() {\n\
             assert_eq!(stable_value(), 10);\n\
         }\n",
    )
    .unwrap();
}

fn run_kiss_check_rust(home: &TempDir, repo: &TempDir) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("check")
        .arg("--lang")
        .arg("rust")
        .arg(repo.path())
        .current_dir(repo.path())
        .env("HOME", home.path())
        .env_remove("LLVM_PROFILE_FILE")
        .output()
        .expect("kiss check should run")
}

fn assert_success(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
