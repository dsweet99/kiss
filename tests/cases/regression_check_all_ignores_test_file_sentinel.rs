use crate::common::seed_python_runtime_coverage;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn write_corpus(dir: &std::path::Path) {
    fs::write(dir.join("lib.py"), "def add(a, b):\n    return a + b\n").unwrap();
    fs::create_dir_all(dir.join("tests")).unwrap();
    fs::write(
        dir.join("tests/test_lib.py"),
        "from lib import add\n\ndef test_add():\n    assert add(1, 2) == 3\n",
    )
    .unwrap();
    fs::write(
        dir.join("pyproject.toml"),
        "[tool.pytest.ini_options]\ntestpaths = [\"tests\"]\npythonpath = [\".\"]\n",
    )
    .unwrap();
    fs::write(
        dir.join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         orphan_module_enabled = false\n\
         \n\
[test]\n\
         test_coverage_threshold = 90\n\
         \n\
         [thresholds]\n\
         statements_per_function = 100\n\
         lines_per_file = 1000\n\
         statements_per_file = 1000\n\
         functions_per_file = 100\n\
         imported_names_per_file = 100\n\
         arguments_positional = 100\n\
         arguments_keyword_only = 100\n\
         max_indentation_depth = 100\n\
         interface_types_per_file = 100\n\
         concrete_types_per_file = 100\n\
         nested_function_depth = 100\n\
         returns_per_function = 100\n\
         return_values_per_function = 100\n\
         branches_per_function = 100\n\
         local_variables_per_function = 100\n\
         statements_per_try_block = 100\n\
         boolean_parameters = 100\n\
         annotations_per_function = 100\n\
         calls_per_function = 100\n\
         methods_per_class = 100\n\
         cycle_size = 100\n\
         indirect_dependencies = 100\n\
         dependency_depth = 100\n",
    )
    .unwrap();
}

#[test]
fn kiss_check_all_passes_without_test_module_violations() {
    let corpus = TempDir::new().unwrap();
    write_corpus(corpus.path());
    seed_python_runtime_coverage(
        corpus.path(),
        &[("tests/test_lib.py::test_add", vec![("lib.py", vec![1, 2])])],
    );

    let home = TempDir::new().unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("__coverage")
        .arg("--all")
        .arg(corpus.path())
        .env("HOME", home.path())
        .output()
        .expect("kiss __coverage --all should run");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "kiss __coverage --all failed (exit {:?})\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code(),
    );
    assert!(
        !stdout.contains("tests/test_lib.py"),
        "expected no test-module coverage violations, got:\n{stdout}"
    );
}
