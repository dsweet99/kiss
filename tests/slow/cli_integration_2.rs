use super::cli_integration::create_god_class_file;
use crate::common::seed_python_runtime_coverage;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn cli_with_lang_filter_python() {
    let tmp = TempDir::new().unwrap();
    create_god_class_file(tmp.path());
    seed_python_runtime_coverage(tmp.path(), &[("tests/test_god.py::test_god", vec![])]);
    let output = kiss_binary()
        .arg("check")
        .arg(tmp.path())
        .arg("--lang")
        .arg("python")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.is_empty() && stdout.contains("VIOLATION"),
        "kiss --lang python should report violations. stdout: {stdout}"
    );
}

#[test]
fn cli_with_lang_filter_rust() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("foo.py"), "def foo(): pass").unwrap();
    let output = kiss_binary()
        .arg("check")
        .arg(tmp.path())
        .arg("--lang")
        .arg("rust")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No Rust files") || stdout.contains("No files"),
        "Should report no Rust files. stdout: {stdout}"
    );
}

#[test]
fn cli_help_flag_works() {
    let output = kiss_binary().arg("--help").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("kiss"),
        "help output should contain 'kiss'. stdout: {stdout}"
    );
}

#[test]
fn cli_version_flag_works() {
    let output = kiss_binary().arg("--version").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("kiss"),
        "version output should contain 'kiss'. stdout: {stdout}"
    );
}

#[test]
fn cli_invalid_lang_reports_error() {
    let output = kiss_binary()
        .arg("check")
        .arg(".")
        .arg("--lang")
        .arg("invalid_language")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("Unknown language") || stderr.contains("error"),
        "Should report unknown language error. stderr: {stderr}"
    );
}

#[test]
fn cli_on_empty_directory() {
    let tmp = TempDir::new().unwrap();
    let output = kiss_binary().arg("check").arg(tmp.path()).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No files") || stdout.contains("No Python") || stdout.contains("No Rust"),
        "Should report no files. stdout: {stdout}"
    );
}

#[test]
fn cli_mimic_command_runs() {
    for args in [vec!["init"], vec!["mimic", "."], vec!["clamp"], vec!["mv"]] {
        let output = kiss_binary().args(&args).output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "removed command {args:?} should fail. stderr: {stderr}"
        );
        assert!(
            stderr.contains("unrecognized subcommand") || stderr.contains("error"),
            "removed command {args:?} should be rejected. stderr: {stderr}"
        );
    }
}

fn write_three_python_files(dir: &std::path::Path) {
    fs::write(dir.join("a.py"), "import b\nimport c\n").unwrap();
    fs::write(dir.join("b.py"), "import c\n").unwrap();
    fs::write(dir.join("c.py"), "def f():\n    return 1\n").unwrap();
}

#[test]
fn cli_viz_num_nodes_one_collapses_to_supernode() {
    let tmp = TempDir::new().unwrap();
    write_three_python_files(tmp.path());

    let out_path = tmp.path().join("graph.mmd");
    let output = kiss_binary()
        .arg("viz")
        .arg(&out_path)
        .arg(tmp.path())
        .arg("--num-nodes=1")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "viz --num-nodes=1 should succeed. stderr: {stderr}"
    );

    let mmd = fs::read_to_string(&out_path).unwrap();
    let node_lines = mmd
        .lines()
        .filter(|l| l.trim_start().starts_with('c') && l.contains('['))
        .count();
    let edge_lines = mmd.lines().filter(|l| l.contains("-->")).count();
    assert_eq!(node_lines, 1, "mmd:\n{mmd}");
    assert_eq!(edge_lines, 0, "mmd:\n{mmd}");
    assert!(mmd.contains("codebase"), "mmd:\n{mmd}");
}

#[test]
fn cli_viz_num_nodes_caps_node_count() {
    let tmp = TempDir::new().unwrap();
    write_three_python_files(tmp.path());

    let out_path = tmp.path().join("graph.mmd");
    let output = kiss_binary()
        .arg("viz")
        .arg(&out_path)
        .arg(tmp.path())
        .arg("--num-nodes=2")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "viz --num-nodes=2 should succeed. stderr: {stderr}"
    );

    let mmd = fs::read_to_string(&out_path).unwrap();
    let node_lines = mmd
        .lines()
        .filter(|l| l.trim_start().starts_with('c') && l.contains('['))
        .count();
    assert!(
        node_lines <= 2,
        "expected at most 2 coarsened nodes, got {node_lines}. mmd:\n{mmd}"
    );
    assert!(node_lines >= 1, "mmd:\n{mmd}");
}

#[test]
fn cli_viz_rejects_zoom_and_num_nodes_together() {
    let tmp = TempDir::new().unwrap();
    write_three_python_files(tmp.path());

    let out_path = tmp.path().join("graph.mmd");
    let output = kiss_binary()
        .arg("viz")
        .arg(&out_path)
        .arg(tmp.path())
        .arg("--zoom=0.5")
        .arg("--num-nodes=2")
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "viz should reject --zoom and --num-nodes together"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflict"),
        "stderr should mention the conflict. stderr:\n{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn target_runner_list_stdout_has_no_cli_wall_timing() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new().unwrap();
    let output_dir = tmp.path().join("instances");
    fs::create_dir_all(&output_dir).unwrap();
    let runner_map = tmp.path().join("runner-map.json");
    fs::write(&runner_map, b"{\"x86_64-unknown-linux-gnu\":[]}").unwrap();
    let script = tmp.path().join("list-child.sh");
    fs::write(
        &script,
        "#!/bin/sh\nprintf 'core_sources_do_not_depend_on_python_boundary_types: test\\n'\nexit 0\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).unwrap();

    let output = kiss_binary()
        .args([
            kiss::rust_llvm_cov_runner::TARGET_RUNNER_SHIM_SUBCOMMAND,
            "--output-dir",
        ])
        .arg(&output_dir)
        .arg("--runner-map")
        .arg(&runner_map)
        .arg("--platform")
        .arg("x86_64-unknown-linux-gnu")
        .arg(&script)
        .env("NEXTEST_TEST_PHASE", "list")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "target runner list should succeed. stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("core_sources_do_not_depend_on_python_boundary_types: test"),
        "list stdout must keep the child test line. stdout:\n{stdout}"
    );
    for line in stdout.lines().filter(|line| !line.is_empty()) {
        assert!(
            line.ends_with(": test") || line.ends_with(": benchmark"),
            "bug_report.md: nextest rejects list line {line:?} in {stdout:?}"
        );
    }
}
