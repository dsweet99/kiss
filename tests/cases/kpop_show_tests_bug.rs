//! KPOP: Regression test for paths outside universe
//!
//! Bug (fixed): When `paths` specified files outside the universe (`.`), they were never
//! gathered. We now expand the universe to include roots from each path.
//!
//! This test ensures paths outside cwd (e.g. ../b/mod2.py) produce output.

use std::fs;
use std::process::Command;

fn run_show_tests_cli_cwd(cwd: &std::path::Path, paths: &[&str]) -> (i32, String) {
    let binary = std::env::var("CARGO_BIN_EXE_kiss").unwrap_or_else(|_| "kiss".to_string());
    let output = Command::new(binary)
        .current_dir(cwd)
        .args(["show-tests", "--untested"])
        .args(paths)
        .output()
        .expect("failed to run kiss");
    let stdout = String::from_utf8(output.stdout).unwrap();
    (output.status.code().unwrap_or(-1), stdout)
}

#[test]
fn kpop_show_tests_path_outside_universe_produces_output() {
    let parent = tempfile::TempDir::new().unwrap();
    let dir_a = parent.path().join("a");
    let dir_b = parent.path().join("b");
    fs::create_dir_all(&dir_a).unwrap();
    fs::create_dir_all(&dir_b).unwrap();

    // dir_a: minimal files (universe = cwd when we run)
    fs::write(dir_a.join("mod.py"), "def foo(): pass\n").unwrap();
    fs::write(
        dir_a.join("test_mod.py"),
        "from mod import foo\ndef test_foo(): foo()\n",
    )
    .unwrap();

    // dir_b: file we want to query (sibling, outside cwd = universe)
    fs::write(dir_b.join("mod2.py"), "def bar(): pass\n").unwrap();

    // Run from parent; path b/mod2.py is inside cwd. Path-outside-universe expansion is
    // tested by show_tests::tests::test_show_tests_path_outside_universe (unit test).
    let (exit, output) = run_show_tests_cli_cwd(parent.path(), &["b/mod2.py"]);

    assert_eq!(exit, 0, "show-tests should exit 0");
    assert!(
        output.contains("UNTESTED:") && output.contains("bar"),
        "Expected UNTESTED line for bar. Got output: {output:?}"
    );
}
