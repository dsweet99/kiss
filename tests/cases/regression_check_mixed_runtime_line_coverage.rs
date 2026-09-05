use crate::common::{generate_lockfile, seed_python_runtime_coverage, seed_rust_runtime_coverage};
use crate::support::git::{commit_all, init_git_repo};
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn mixed_python_and_rust_runtime_line_coverage_can_pass_together() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    init_git_repo(repo.path());
    write_mixed_runtime_repo(&repo);
    generate_lockfile(repo.path());
    seed_python_runtime_coverage(
        repo.path(),
        &[("test_app.py::test_py_value", vec![("app.py", vec![1, 2])])],
    );
    seed_rust_runtime_coverage(
        repo.path(),
        &[(
            "tests::test_rust_value",
            vec![("src/lib.rs", (1_u32..=13).collect())],
        )],
    );
    commit_all(repo.path(), "init");

    let out = run_kiss_cov_all(&home, &repo);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        out.status.success(),
        "mixed runtime coverage should pass. stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("VIOLATION:test_coverage"),
        "fully covered mixed snapshot should not report coverage violations. stdout:\n{stdout}"
    );
}

fn write_mixed_runtime_repo(repo: &TempDir) {
    fs::create_dir_all(repo.path().join("src")).unwrap();
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
        repo.path().join("app.py"),
        "def py_value():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("test_app.py"),
        "from app import py_value\n\ndef test_py_value():\n    assert py_value() == 1\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"mixed_runtime_coverage\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src").join("lib.rs"),
        "pub fn rust_value() -> i32 {\n\
             1\n\
         }\n\n\
         #[cfg(test)]\n\
         mod tests {\n\
             use super::rust_value;\n\n\
             #[test]\n\
             fn test_rust_value() {\n\
                 assert_eq!(rust_value(), 1);\n\
             }\n\
         }\n",
    )
    .unwrap();
}

fn run_kiss_cov_all(home: &TempDir, repo: &TempDir) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
        .current_dir(repo.path())
        .arg("test")
        .arg("--coverage-all")
        .arg(".")
        .env("HOME", home.path())
        .output()
        .expect("kiss test should run")
}
