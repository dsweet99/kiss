use crate::common::seed_python_runtime_coverage;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn write_docs_config(root: &std::path::Path, docs_allowed: &str) {
    fs::write(
        root.join(".kissconfig"),
        format!(
            "[global]\n\
             duplication_enabled = false\n\
             orphan_module_enabled = false\n\
             comment_removal_enabled = false\n\
             docs_allowed = {docs_allowed}\n\
             \n\
             [test]\n\
             test_coverage_threshold = 0\n"
        ),
    )
    .unwrap();
}

#[test]
fn check_flags_python_docstring_outside_docs_allowed() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/ok.py"),
        "\"\"\"allowed\"\"\"\n\ndef foo():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        root.join("app.py"),
        "\"\"\"not allowed\"\"\"\n\ndef foo():\n    return 1\n",
    )
    .unwrap();
    seed_python_runtime_coverage(
        root,
        &[
            ("tests/test_app.py::test_app", vec![]),
            ("tests/test_ok.py::test_ok", vec![]),
        ],
    );
    write_docs_config(root, "[\"docs\"]");
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
        stdout.contains("VIOLATION:doc:") && stdout.contains("app.py"),
        "expected doc violation for app.py; stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("ok.py"),
        "docs/ok.py should be allowed; stdout:\n{stdout}"
    );
}

#[test]
fn check_ignores_docs_when_docs_allowed_is_empty() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(
        root.join("app.py"),
        "\"\"\"module doc\"\"\"\n\ndef foo():\n    return 1\n",
    )
    .unwrap();
    seed_python_runtime_coverage(root, &[("tests/test_app.py::test_app", vec![])]);
    write_docs_config(root, "[]");
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
        stdout.contains("VIOLATION:doc:") && stdout.contains("app.py"),
        "empty docs_allowed must flag docs; stdout:\n{stdout}"
    );
}

#[test]
fn check_flags_rust_doc_comment_outside_docs_allowed() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("docs")).unwrap();
    fs::write(root.join("docs/ok.rs"), "/// allowed\nfn foo() {}\n").unwrap();
    fs::write(root.join("lib.rs"), "/// not allowed\nfn foo() {}\n").unwrap();
    write_docs_config(root, "[\"docs\"]");
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
        stdout.contains("VIOLATION:doc:") && stdout.contains("lib.rs"),
        "expected rust doc violation; stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("ok.rs"),
        "docs/ok.rs should be allowed; stdout:\n{stdout}"
    );
}

#[test]
fn library_emits_docs_allowed_in_generated_config() {
    let gate = kiss::GateConfig {
        docs_allowed: vec!["docs".to_string()],
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
    assert!(toml.contains("docs_allowed = [\"docs\"]"), "toml:\n{toml}");
    let py = crate::common::parse_python_source("\"\"\"d\"\"\"\nx = 1\n");
    let viols = kiss::collect_doc_violations(
        &[py],
        &[],
        &["nowhere".to_string()],
        std::path::Path::new("."),
    );
    assert_eq!(viols[0].metric, kiss::DOC_METRIC);
}

#[test]
fn check_does_not_treat_host_tmp_or_nested_src_as_allowed() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("vendor/src")).unwrap();
    fs::write(
        root.join("app.py"),
        "\"\"\"not allowed by host tmp\"\"\"\n\ndef foo():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        root.join("vendor/src/lib.rs"),
        "/// nested src\nfn foo() {}\n",
    )
    .unwrap();
    seed_python_runtime_coverage(root, &[("tests/test_app.py::test_app", vec![])]);
    write_docs_config(root, "[\"tmp\", \"src\"]");
    let py = kiss_binary()
        .current_dir(root)
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg(".")
        .output()
        .expect("kiss check python should run");
    let py_out = String::from_utf8_lossy(&py.stdout);
    assert!(
        py_out.contains("VIOLATION:doc:") && py_out.contains("app.py"),
        "host /tmp must not satisfy docs_allowed=[\"tmp\"]; stdout:\n{py_out}"
    );
    let rs = kiss_binary()
        .current_dir(root)
        .arg("check")
        .arg("--lang")
        .arg("rust")
        .arg(".")
        .output()
        .expect("kiss check rust should run");
    let rs_out = String::from_utf8_lossy(&rs.stdout);
    assert!(
        rs_out.contains("VIOLATION:doc:") && rs_out.contains("lib.rs"),
        "vendor/src must not satisfy docs_allowed=[\"src\"]; stdout:\n{rs_out}"
    );
}
