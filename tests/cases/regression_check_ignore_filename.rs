
use crate::common::seed_python_runtime_coverage;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn write_trivial_py(path: &std::path::Path) {
    fs::write(path, "def f():\n    return 1\n").unwrap();
}

#[test]
fn cli_check_ignore_excludes_matching_directory() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("subdir")).unwrap();
    write_trivial_py(&root.join("keep.py"));
    write_trivial_py(&root.join("subdir").join("drop.py"));
    seed_python_runtime_coverage(root, &[("test_ignore.py::test_ignore", vec![])]);

    let baseline = kiss_binary()
        .arg("check")
        .arg(root)
        .output()
        .unwrap();
    let baseline_stdout = String::from_utf8_lossy(&baseline.stdout);
    assert!(
        baseline_stdout.contains("Analyzed: 2 files"),
        "baseline should see both files. stdout:\n{baseline_stdout}"
    );

    let filtered = kiss_binary()
        .arg("check")
        .arg("--ignore=subdir")
        .arg(root)
        .output()
        .unwrap();
    let filtered_stdout = String::from_utf8_lossy(&filtered.stdout);
    assert!(
        filtered_stdout.contains("Analyzed: 1 files"),
        "--ignore=subdir should drop the subdir file. stdout:\n{filtered_stdout}"
    );
}

#[test]
fn cli_check_ignore_excludes_matching_filename() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_trivial_py(&root.join("big.py"));
    write_trivial_py(&root.join("small.py"));
    seed_python_runtime_coverage(root, &[("test_ignore.py::test_ignore", vec![])]);

    let baseline = kiss_binary()
        .arg("check")
        .arg(root)
        .output()
        .unwrap();
    let baseline_stdout = String::from_utf8_lossy(&baseline.stdout);
    assert!(
        baseline_stdout.contains("Analyzed: 2 files"),
        "baseline should see both files. stdout:\n{baseline_stdout}"
    );

    let filtered = kiss_binary()
        .arg("check")
        .arg("--ignore=big")
        .arg(root)
        .output()
        .unwrap();
    let filtered_stdout = String::from_utf8_lossy(&filtered.stdout);
    let filtered_stderr = String::from_utf8_lossy(&filtered.stderr);
    assert!(
        filtered_stdout.contains("Analyzed: 1 files"),
        "--ignore=big should drop big.py per documented behavior \
         (\"Ignore files/directories starting with PREFIX\"). \
         stdout:\n{filtered_stdout}\nstderr:\n{filtered_stderr}"
    );
    assert!(
        !filtered_stdout.contains("big.py"),
        "big.py should not appear in violations after --ignore=big. \
         stdout:\n{filtered_stdout}"
    );
}
