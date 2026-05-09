use super::cli_integration::create_god_class_file;
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
    let output = kiss_binary()
        .arg("check")
        .arg(tmp.path())
        .arg("--lang")
        .arg("python")
        .arg("--all")
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
        .arg("--all")
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
    let output = kiss_binary()
        .arg("check")
        .arg(tmp.path())
        .arg("--all")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No files") || stdout.contains("No Python") || stdout.contains("No Rust"),
        "Should report no files. stdout: {stdout}"
    );
}

#[test]
fn cli_mimic_command_runs() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("mod.py"), "def foo(): x = 1").unwrap();
    let output = kiss_binary().arg("mimic").arg(tmp.path()).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[python]") || stdout.contains("Generated"),
        "kiss mimic should produce config. stdout: {stdout}"
    );
}

#[test]
fn cli_mv_dry_run_emits_human_plan_lines() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("mod.py");
    fs::write(&source, "def foo():\n    return 1\nfoo()\n").unwrap();

    let output = kiss_binary()
        .arg("mv")
        .arg(format!("{}::foo", source.display()))
        .arg("bar")
        .arg(tmp.path())
        .arg("--dry-run")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "mv dry-run should succeed. stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("foo -> bar"),
        "expected rename plan line. stdout:\n{stdout}"
    );
}

#[test]
fn cli_mv_json_emits_stable_schema() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("mod.py");
    fs::write(&source, "def foo():\n    return foo()\n").unwrap();

    let output = kiss_binary()
        .arg("mv")
        .arg(format!("{}::foo", source.display()))
        .arg("bar")
        .arg(tmp.path())
        .arg("--dry-run")
        .arg("--json")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "mv json should succeed. stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    assert!(stdout.contains("\"files\""), "stdout:\n{stdout}");
    assert!(stdout.contains("\"edits\""), "stdout:\n{stdout}");
    assert!(stdout.contains("\"old_snippet\""), "stdout:\n{stdout}");
    assert!(stdout.contains("\"new_snippet\""), "stdout:\n{stdout}");
}

#[test]
fn cli_mv_requires_query_shape() {
    let output = kiss_binary()
        .arg("mv")
        .arg("bad_query")
        .arg("bar")
        .arg("--dry-run")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "mv should fail for bad query");
    assert!(
        stderr.contains("source must contain '::'"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn cli_mv_parse_failure_warning_includes_source_path() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("broken.rs");
    let other = tmp.path().join("good.rs");
    fs::write(&source, "fn main( {\n").unwrap();
    fs::write(&other, "fn helper() {}\n").unwrap();

    let output = kiss_binary()
        .arg("mv")
        .arg(format!("{}::main", source.display()))
        .arg("main2")
        .arg(tmp.path())
        .arg("--dry-run")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("kiss mv: {}:", source.display())),
        "warning should include failed source path. stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("skipping file"),
        "warning should announce that the file is skipped. stderr:\n{stderr}"
    );
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
