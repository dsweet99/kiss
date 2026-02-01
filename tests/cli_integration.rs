use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn create_god_class_file(dir: &std::path::Path) {
    let content = r"class GodClass:
    def m1(self): pass
    def m2(self): pass
    def m3(self): pass
    def m4(self): pass
    def m5(self): pass
    def m6(self): pass
    def m7(self): pass
    def m8(self): pass
    def m9(self): pass
    def m10(self): pass
    def m11(self): pass
    def m12(self): pass
    def m13(self): pass
    def m14(self): pass
    def m15(self): pass
    def m16(self): pass
    def m17(self): pass
    def m18(self): pass
    def m19(self): pass
    def m20(self): pass
    def m21(self): pass
";
    fs::write(dir.join("god_class.py"), content).unwrap();
}

#[test]
fn cli_analyze_runs_on_python() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("simple.py"), "def foo(): pass").unwrap();
    let output = kiss_binary()
        .arg("check")
        .arg(tmp.path())
        .arg("--all")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() || !stdout.is_empty(),
        "kiss should run. stdout: {stdout}"
    );
}

#[test]
fn cli_analyze_reports_violations_on_god_class() {
    let tmp = TempDir::new().unwrap();
    create_god_class_file(tmp.path());
    let output = kiss_binary()
        .arg("check")
        .arg(tmp.path())
        .arg("--all")
        .arg("--lang")
        .arg("python")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("VIOLATION") || stdout.contains("methods"),
        "god_class should report violations. stdout: {stdout}"
    );
}

#[test]
fn cli_stats_command_runs() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("mod.py"), "def foo(): x = 1").unwrap();
    let output = kiss_binary().arg("stats").arg(tmp.path()).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("stats") || stdout.contains("Python") || stdout.contains("files"),
        "kiss stats should produce output. stdout: {stdout}"
    );
}

#[test]
fn cli_stats_all_uses_metric_registry_display_names() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("mod.py"),
        "def foo(a, b):\n    return a + b\n",
    )
    .unwrap();
    let output = kiss_binary()
        .arg("stats")
        .arg(tmp.path())
        .arg("--all")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Regression: `--all` should use the same display names as the summary registry.
    assert!(
        stdout.contains("args_total  (Arguments (total))"),
        "Expected args_total display name to match summary. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("imported_names_per_file  (Imported names per file)"),
        "Expected imported_names_per_file display name to match summary. stdout:\n{stdout}"
    );
}

#[test]
fn cli_stats_all_does_not_consume_path_as_n() {
    // Regression: `--all` takes an optional N, but it must not steal the first PATH argument.
    // Users should be able to run: `kiss stats --all <path>` without needing `--`.
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("mod.py"), "def foo():\n    return 1\n").unwrap();
    let output = kiss_binary()
        .arg("stats")
        .arg("--all")
        .arg(tmp.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Expected success. stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("kiss stats --all"),
        "Expected --all header. stdout:\n{stdout}"
    );
}

#[test]
fn cli_stats_summary_includes_lines_per_file() {
    // Regression: `--all` already reports `lines_per_file`; the summary should include it too
    // so that users see the same metric set across views.
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("mod.py"), "def foo():\n    return 1\n").unwrap();
    let output = kiss_binary().arg("stats").arg(tmp.path()).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("lines_per_file"),
        "Expected summary to include lines_per_file. stdout:\n{stdout}"
    );
}

#[test]
fn cli_viz_writes_dot_file() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.py"), "import b\n").unwrap();
    fs::write(tmp.path().join("b.py"), "def f():\n    return 1\n").unwrap();

    let out_path = tmp.path().join("graph.dot");
    let output = kiss_binary()
        .arg("viz")
        .arg(&out_path)
        .arg(tmp.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "viz should succeed. stderr: {stderr}"
    );

    let dot = fs::read_to_string(&out_path).unwrap();
    assert!(dot.contains("digraph kiss"), "dot:\n{dot}");
    // At least one edge should exist.
    assert!(dot.contains("->"), "dot:\n{dot}");
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
    assert!(stdout.contains("kiss") || stdout.contains("Code-quality"));
}

#[test]
fn cli_version_flag_works() {
    let output = kiss_binary().arg("--version").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("kiss") || stdout.contains("0."));
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
