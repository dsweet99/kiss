use crate::common::seed_python_runtime_coverage;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn write_violating_py(path: &std::path::Path) {
    fs::write(path, "def big(a, b, c, d, e, f, g, h):\n    return a\n").unwrap();
}

#[test]
fn cli_check_focus_dir_with_source_restricts_report() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("focus_dir")).unwrap();
    write_violating_py(&root.join("src").join("big.py"));
    fs::write(
        root.join("focus_dir").join("ok.py"),
        "def g(x):\n    return x\n",
    )
    .unwrap();
    seed_python_runtime_coverage(root, &[("test_focus.py::test_focus", vec![])]);
    let config = crate::common::write_builtin_language_config(root);

    let out = kiss_binary()
        .arg("check")
        .arg("--config")
        .arg(&config)
        .arg(root)
        .arg(root.join("focus_dir"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("big.py"),
        "focus=focus_dir/ should hide src/big.py violations. stdout:\n{stdout}"
    );
}

#[test]
fn cli_check_focus_dir_with_no_source_does_not_leak_universe() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("non_src")).unwrap();
    write_violating_py(&root.join("src").join("big.py"));
    fs::write(root.join("non_src").join("readme.txt"), "hello\n").unwrap();
    seed_python_runtime_coverage(root, &[("test_focus.py::test_focus", vec![])]);
    let config = crate::common::write_builtin_language_config(root);

    let universe_only = kiss_binary()
        .arg("check")
        .arg("--config")
        .arg(&config)
        .arg(root)
        .output()
        .unwrap();
    let universe_stdout = String::from_utf8_lossy(&universe_only.stdout);
    assert!(
        universe_stdout.contains("big.py"),
        "sanity: universe-only run should report big.py. stdout:\n{universe_stdout}"
    );

    let focused = kiss_binary()
        .arg("check")
        .arg("--config")
        .arg(&config)
        .arg(root)
        .arg(root.join("non_src"))
        .output()
        .unwrap();
    let focused_stdout = String::from_utf8_lossy(&focused.stdout);
    let focused_stderr = String::from_utf8_lossy(&focused.stderr);
    assert!(
        !focused_stdout.contains("big.py"),
        "focus=non_src/ (no source files) must not leak src/big.py violations. \
         stdout:\n{focused_stdout}\nstderr:\n{focused_stderr}"
    );
}

#[test]
fn cli_check_requires_runtime_coverage_for_universe_languages_before_focus() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("app.py"), "def covered():\n    return 1\n").unwrap();
    fs::write(
        root.join("test_app.py"),
        "from app import covered\n\ndef test_app():\n    assert covered() == 1\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"mixed_focus\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src").join("lib.rs"),
        "pub fn uncovered() -> i32 { 1 }\n",
    )
    .unwrap();
    seed_python_runtime_coverage(
        root,
        &[("test_app.py::test_app", vec![("app.py", vec![1, 2])])],
    );

    let focused = kiss_binary()
        .arg("__coverage")
        .arg("--all")
        .arg(root)
        .arg(root.join("app.py"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&focused.stdout);
    let stderr = String::from_utf8_lossy(&focused.stderr);

    assert_eq!(
        focused.status.code(),
        Some(1),
        "missing Rust runtime coverage should fail even when focus is Python-only. \
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Rust runtime line coverage") && !stderr.contains("kiss test commit"),
        "error should identify Rust runtime coverage without the old manual refresh instruction. \
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
