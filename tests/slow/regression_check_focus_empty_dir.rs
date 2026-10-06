use crate::common::seed_python_runtime_coverage;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiss"));
    crate::common::scrub_parent_coverage_env(&mut cmd);
    crate::common::preserve_toolchain_homes(&mut cmd);
    cmd
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
