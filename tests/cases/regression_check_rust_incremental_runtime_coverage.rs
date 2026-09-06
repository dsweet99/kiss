use crate::common::generate_lockfile;
use crate::support::git::git_command;
use serde_json::Value;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn rust_runtime_coverage_refresh_publishes_incremental_generation() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    init_git_repo(repo.path());
    write_incremental_rust_repo(&repo, "1");
    generate_lockfile(repo.path());

    let cold = run_kiss_test_rust(home.path(), repo.path());
    assert_success("cold kiss test", &cold);
    assert_eq!(
        passed_selector_entry_count(repo.path()),
        4,
        "forced kiss test should publish four passed entries"
    );

    write_incremental_rust_repo(&repo, "2");
    let incremental = run_kiss_test_rust(home.path(), repo.path());
    assert_success("incremental kiss test", &incremental);
    assert_eq!(
        passed_selector_entry_count(repo.path()),
        4,
        "editing one package should still publish four passed entries"
    );

    let warm = run_kiss_test_rust(home.path(), repo.path());
    assert_success("warm kiss test", &warm);
}

fn init_git_repo(repo: &std::path::Path) {
    assert!(git_command(repo).args(["init"]).status().unwrap().success());
    for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
        assert!(git_command(repo)
            .args(["config", kv.0, kv.1])
            .status()
            .unwrap()
            .success());
    }
    assert!(git_command(repo)
        .args(["commit", "--allow-empty", "-m", "init"])
        .status()
        .unwrap()
        .success());
}

fn passed_selector_entry_count(repo: &std::path::Path) -> usize {
    let entries = repo.join(".kiss/rust_llvm_cov_cache/entries");
    fs::read_dir(entries)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension().and_then(|ext| ext.to_str()) == Some("json")).then_some(path)
        })
        .filter(|path| {
            let raw: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            raw["status"] == "Passed"
        })
        .count()
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
        "[global]\n\
         duplication_enabled = false\n\
\n\
[test]\n\
         test_coverage_threshold = 100\n\
         [python]\n\
         [rust]\n",
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
             fn covers_lib_first() {{\n\
                 assert_eq!(value(), {value});\n\
             }}\n\n\
             #[test]\n\
             fn covers_lib_second() {{\n\
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
         fn covers_stable_first() {\n\
             assert_eq!(stable_value(), 10);\n\
         }\n\n\
         #[test]\n\
         fn covers_stable_second() {\n\
             assert_eq!(stable_value(), 10);\n\
         }\n",
    )
    .unwrap();
}

fn run_kiss_test_rust(home: &std::path::Path, repo: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("--lang")
        .arg("rust")
        .arg("test")
        .arg(".")
        .current_dir(repo)
        .env("HOME", home)
        .env_remove("LLVM_PROFILE_FILE")
        .output()
        .expect("kiss test should run")
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
