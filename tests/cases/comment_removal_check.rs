use crate::common::{parse_python_source, seed_python_runtime_coverage};
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn write_gate_config(root: &std::path::Path, enabled: bool) {
    fs::write(
        root.join(".kissconfig"),
        format!(
            "[global]\n\
             duplication_enabled = false\n\
             orphan_module_enabled = false\n\
             comment_removal_enabled = {enabled}\n\
             \n\
             [test]\n\
             test_coverage_threshold = 0\n"
        ),
    )
    .unwrap();
}

#[test]
fn check_flags_python_comment_not_docstring_when_enabled() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(
        root.join("app.py"),
        "\"\"\"module doc\"\"\"\n\ndef foo():\n    \"\"\"fn doc\"\"\"\n    # remove me\n    return 1\n",
    )
    .unwrap();
    seed_python_runtime_coverage(root, &[("tests/test_app.py::test_app", vec![])]);
    write_gate_config(root, true);
    let out = kiss_binary()
        .current_dir(root)
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg(".")
        .output()
        .expect("kiss check should run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("VIOLATION:comment:") && stdout.contains("app.py"),
        "expected comment violation; stdout:\n{stdout}"
    );
    assert!(
        !out.status.success(),
        "comment violation should fail check; stdout:\n{stdout}"
    );
}

#[test]
fn check_ignores_python_comments_when_disabled() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(root.join("app.py"), "# stay\ndef foo():\n    return 1\n").unwrap();
    seed_python_runtime_coverage(root, &[("tests/test_app.py::test_app", vec![])]);
    write_gate_config(root, false);
    let out = kiss_binary()
        .current_dir(root)
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg(".")
        .output()
        .expect("kiss check should run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("VIOLATION:comment:"),
        "disabled gate must not flag comments; stdout:\n{stdout}"
    );
}

#[test]
fn check_flags_rust_line_comment_not_doc_when_enabled() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(
        root.join("lib.rs"),
        "/// docs\nfn foo() {\n    // remove me\n}\n",
    )
    .unwrap();
    write_gate_config(root, true);
    let out = kiss_binary()
        .current_dir(root)
        .arg("check")
        .arg("--lang")
        .arg("rust")
        .arg(".")
        .output()
        .expect("kiss check should run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("VIOLATION:comment:") && stdout.contains("lib.rs"),
        "expected rust comment violation; stdout:\n{stdout}"
    );
}

#[test]
fn library_collects_comments_and_emits_config_flag() {
    let py = parse_python_source("# n\nx = 1\n");
    assert!(kiss::has_non_doc_comments(&[py], &[]));
    let tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    fs::write(tmp.path(), "// n\nfn f() {}\n").unwrap();
    let rs = kiss::parse_rust_file(tmp.path()).unwrap();
    let viols = kiss::collect_comment_violations(&[], &[rs]);
    assert_eq!(viols[0].metric, kiss::COMMENT_METRIC);
    let gate = kiss::GateConfig {
        comment_removal_enabled: true,
        ..kiss::GateConfig::default()
    };
    let toml = kiss::config_gen::generate_config_toml_by_language(
        &kiss::config_gen::GenerateConfigParams {
            py: &kiss::MetricStats::default(),
            rs: &kiss::MetricStats::default(),
            py_n: 0,
            rs_n: 0,
            gate: &gate,
        },
    );
    assert!(toml.contains("comment_removal_enabled = true"));
}
