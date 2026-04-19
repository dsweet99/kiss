use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn cli_init_writes_default_config_in_current_directory() {
    let tmp = TempDir::new().unwrap();
    let output = kiss_binary().arg("init").current_dir(tmp.path()).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "init should succeed. stderr:\n{stderr}\nstdout:\n{stdout}"
    );

    let config = fs::read_to_string(tmp.path().join(".kissconfig")).unwrap();
    assert!(
        config.contains("[gate]\ntest_coverage_threshold = 90\nmin_similarity = 0.7\nduplication_enabled = true\norphan_module_enabled = true"),
        "config:\n{config}"
    );
    assert!(
        config.contains("[python]\nstatements_per_function = 35\npositional_args = 3\nkeyword_only_args = 3"),
        "config:\n{config}"
    );
    assert!(
        config.contains("[rust]\nstatements_per_function = 35\narguments = 8"),
        "config:\n{config}"
    );
    assert!(
        stdout.contains("Wrote default config"),
        "stdout should confirm the write. stdout:\n{stdout}"
    );
}

#[test]
fn cli_init_does_not_overwrite_existing_config() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join(".kissconfig");
    fs::write(&config_path, "original = true\n").unwrap();

    let output = kiss_binary().arg("init").arg(tmp.path()).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "init should not fail when config exists. stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    assert_eq!(fs::read_to_string(&config_path).unwrap(), "original = true\n");
    assert!(
        stdout.contains("did not overwrite it"),
        "stdout should explain that the file was preserved. stdout:\n{stdout}"
    );
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

    // `--all` outputs machine-readable STAT lines with metric_id as the key.
    // Format: STAT:<metric_id>:<value>:<file>:<line>:<name>
    // Metric IDs MUST match the canonical registry in `kiss::METRICS`
    // (`src/stats/definitions.rs`); downstream tooling (mimic, .kissconfig,
    // summary tables) joins on those IDs.
    assert!(
        stdout.contains("STAT:arguments_per_function:"),
        "Expected STAT:arguments_per_function line in output. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("STAT:imported_names_per_file:"),
        "Expected STAT:imported_names_per_file line in output. stdout:\n{stdout}"
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
fn cli_viz_writes_mermaid_file() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.py"), "import b\n").unwrap();
    fs::write(tmp.path().join("b.py"), "def f():\n    return 1\n").unwrap();

    let out_path = tmp.path().join("graph.mmd");
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

    let mmd = fs::read_to_string(&out_path).unwrap();
    assert!(mmd.starts_with("graph "), "mmd:\n{mmd}");
    assert!(mmd.contains("-->"), "mmd:\n{mmd}");
}

#[test]
fn cli_viz_writes_markdown_file() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.py"), "import b\n").unwrap();
    fs::write(tmp.path().join("b.py"), "def f():\n    return 1\n").unwrap();

    let out_path = tmp.path().join("graph.md");
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

    let md = fs::read_to_string(&out_path).unwrap();
    assert!(md.starts_with("```mermaid\n"), "md:\n{md}");
    assert!(md.contains("\n```"), "md:\n{md}");
    assert!(md.contains("-->"), "md:\n{md}");
}

#[test]
fn cli_viz_rejects_unknown_output_extension() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.py"), "import b\n").unwrap();
    fs::write(tmp.path().join("b.py"), "def f():\n    return 1\n").unwrap();

    let out_path = tmp.path().join("graph.txt");
    let output = kiss_binary()
        .arg("viz")
        .arg(&out_path)
        .arg(tmp.path())
        .output()
        .unwrap();

    assert!(!output.status.success(), "viz should fail on .txt");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Unsupported output file extension"),
        "stderr: {stderr}"
    );
}

#[test]
fn cli_viz_zoom_zero_collapses_to_one_node() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.py"), "import b\n").unwrap();
    fs::write(tmp.path().join("b.py"), "def f():\n    return 1\n").unwrap();

    let out_path = tmp.path().join("graph.mmd");
    let output = kiss_binary()
        .arg("viz")
        .arg(&out_path)
        .arg(tmp.path())
        .arg("--zoom=0")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "viz should succeed. stderr: {stderr}"
    );

    let mmd = fs::read_to_string(&out_path).unwrap();
    // One node, no edges.
    let node_lines = mmd
        .lines()
        .filter(|l| l.trim_start().starts_with('c') && l.contains('['))
        .count();
    let edge_lines = mmd.lines().filter(|l| l.contains("-->")).count();
    assert_eq!(node_lines, 1, "mmd:\n{mmd}");
    assert_eq!(edge_lines, 0, "mmd:\n{mmd}");
}

#[test]
fn cli_viz_zoom_near_one_is_not_collapsed() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.py"), "import b\n").unwrap();
    fs::write(tmp.path().join("b.py"), "def f():\n    return 1\n").unwrap();

    let out_path = tmp.path().join("graph.mmd");
    let output = kiss_binary()
        .arg("viz")
        .arg(&out_path)
        .arg(tmp.path())
        .arg("--zoom=0.99")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "viz should succeed. stderr: {stderr}"
    );

    let mmd = fs::read_to_string(&out_path).unwrap();
    let node_lines = mmd
        .lines()
        .filter(|l| l.trim_start().starts_with('c') && l.contains('['))
        .count();
    assert!(
        node_lines >= 2,
        "expected zoom=0.99 to keep at least 2 nodes. mmd:\n{mmd}"
    );
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
    assert!(stderr.contains("source must contain '::'"), "stderr:\n{stderr}");
}
