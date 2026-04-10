use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

#[test]
fn test_layout_basic() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("main.py"), "def foo(): pass").unwrap();

    let output = kiss_binary()
        .arg("layout")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "layout should succeed. stderr: {stderr}\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("# Proposed Layout"),
        "Expected markdown header. stdout: {stdout}"
    );
    assert!(
        stdout.contains("## Summary"),
        "Expected summary section. stdout: {stdout}"
    );
}

#[test]
fn test_layout_writes_to_file() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("main.py"), "def foo(): pass").unwrap();

    let out_path = tmp.path().join("layout.md");
    let output = kiss_binary()
        .arg("layout")
        .arg("--out")
        .arg(&out_path)
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "layout --out should succeed. stderr: {stderr}"
    );
    assert!(out_path.exists(), "Output file should be created");

    let contents = fs::read_to_string(&out_path).unwrap();
    assert!(
        contents.contains("# Proposed Layout"),
        "Output file should contain markdown. contents: {contents}"
    );
}

#[test]
fn test_layout_no_files_returns_error() {
    let tmp = TempDir::new().unwrap();

    let output = kiss_binary()
        .arg("layout")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "layout on empty dir should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No source files") || stderr.contains("Error"),
        "Should report no files error. stderr: {stderr}"
    );
}

#[test]
fn test_layout_lang_filter() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("main.py"), "def foo(): pass").unwrap();
    fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();

    // With --lang py, should work (Python file exists)
    let output = kiss_binary()
        .arg("layout")
        .arg("--lang")
        .arg("py")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "layout --lang py should succeed. stderr: {stderr}"
    );
    assert!(
        stdout.contains("# Proposed Layout"),
        "Expected markdown output. stdout: {stdout}"
    );

    // With --lang rust on Python-only dir, should fail
    let tmp_py_only = TempDir::new().unwrap();
    fs::write(tmp_py_only.path().join("main.py"), "def foo(): pass").unwrap();

    let output = kiss_binary()
        .arg("layout")
        .arg("--lang")
        .arg("rust")
        .current_dir(tmp_py_only.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "layout --lang rust on Python-only dir should fail"
    );
}

#[test]
fn test_layout_ignore_prefix() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path().join("vendor");
    fs::create_dir(&vendor).unwrap();
    fs::write(vendor.join("lib.py"), "def vendor_fn(): pass").unwrap();
    fs::write(tmp.path().join("main.py"), "def main(): pass").unwrap();

    // Without --ignore, should find both files
    let output = kiss_binary()
        .arg("layout")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "layout should succeed without ignore"
    );

    // With --ignore vendor, should still work (main.py exists)
    let output = kiss_binary()
        .arg("layout")
        .arg("--ignore")
        .arg("vendor")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "layout --ignore vendor should succeed. stderr: {stderr}"
    );
    assert!(
        stdout.contains("# Proposed Layout"),
        "Expected markdown output. stdout: {stdout}"
    );

    // With --ignore that excludes all files, should fail
    let tmp_vendor_only = TempDir::new().unwrap();
    let vendor_only = tmp_vendor_only.path().join("vendor");
    fs::create_dir(&vendor_only).unwrap();
    fs::write(vendor_only.join("lib.py"), "def vendor_fn(): pass").unwrap();

    let output = kiss_binary()
        .arg("layout")
        .arg("--ignore")
        .arg("vendor")
        .current_dir(tmp_vendor_only.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "layout --ignore vendor (excluding all files) should fail"
    );
}
