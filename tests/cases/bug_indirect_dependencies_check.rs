use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn bug_check_reports_indirect_dependencies_for_fan_in_zero_entry() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(root.join("entry.py"), "import hub\n").unwrap();
    fs::write(root.join("hub.py"), "import leaf\n").unwrap();
    fs::write(root.join("leaf.py"), "VALUE = 1\n").unwrap();
    fs::write(
        root.join(".kissconfig"),
        "[gate]\n\
         test_coverage_threshold = 0\n\
         duplication_enabled = false\n\
         orphan_module_enabled = false\n\
         \n\
         [python]\n\
         indirect_dependencies = 0\n",
    )
    .unwrap();

    let out = kiss_binary()
        .current_dir(root)
        .arg("check")
        .arg("--lang")
        .arg("python")
        .arg("--all")
        .arg(".")
        .output()
        .expect("kiss check should run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("VIOLATION:indirect_dependencies")
            && stdout.contains("entry"),
        "expected indirect_dependencies violation for entry module; stdout:\n{stdout}"
    );
}
