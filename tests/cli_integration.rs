
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
    let output = kiss_binary().arg(tmp.path()).arg("--all").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success() || !stdout.is_empty(), "kiss should run. stdout: {stdout}");
}

#[test]
fn cli_analyze_reports_violations_on_god_class() {
    let tmp = TempDir::new().unwrap();
    create_god_class_file(tmp.path());
    let output = kiss_binary().arg(tmp.path()).arg("--all").arg("--lang").arg("python").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("VIOLATION") || stdout.contains("methods"), "god_class should report violations. stdout: {stdout}");
}

#[test]
fn cli_stats_command_runs() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("mod.py"), "def foo(): x = 1").unwrap();
    let output = kiss_binary().arg("stats").arg(tmp.path()).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("stats") || stdout.contains("Python") || stdout.contains("files"), "kiss stats should produce output. stdout: {stdout}");
}

#[test]
fn cli_with_lang_filter_python() {
    let tmp = TempDir::new().unwrap();
    create_god_class_file(tmp.path());
    let output = kiss_binary().arg(tmp.path()).arg("--lang").arg("python").arg("--all").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty() && stdout.contains("VIOLATION"), "kiss --lang python should report violations. stdout: {stdout}");
}

#[test]
fn cli_with_lang_filter_rust() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("foo.py"), "def foo(): pass").unwrap();
    let output = kiss_binary().arg(tmp.path()).arg("--lang").arg("rust").arg("--all").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No Rust files") || stdout.contains("No files"), "Should report no Rust files. stdout: {stdout}");
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
    let output = kiss_binary().arg("--lang").arg("invalid_language").arg(".").output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("Unknown language") || stderr.contains("error"), "Should report unknown language error. stderr: {stderr}");
}

#[test]
fn cli_on_empty_directory() {
    let tmp = TempDir::new().unwrap();
    let output = kiss_binary().arg(tmp.path()).arg("--all").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No files") || stdout.contains("No Python") || stdout.contains("No Rust"), "Should report no files. stdout: {stdout}");
}

#[test]
fn cli_mimic_command_runs() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("mod.py"), "def foo(): x = 1").unwrap();
    let output = kiss_binary().arg("mimic").arg(tmp.path()).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[python]") || stdout.contains("Generated"), "kiss mimic should produce config. stdout: {stdout}");
}
