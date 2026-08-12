//! Bug: nested non-member Cargo workspaces were scored for by_file coverage
//! while their tests were never executed under the root llvm-cov population.
//! Policy B: exclude those sources from ordinary_source_digests and fail fast
//! when path-targeting them.

use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn kiss_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
}

fn write_nested_workspace_repo(root: &std::path::Path) {
    let member = root.join("member");
    let nested = root.join("nested");
    fs::create_dir_all(member.join("src")).unwrap();
    fs::create_dir_all(nested.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        member.join("src").join("lib.rs"),
        "pub fn member_fn() -> u32 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn covers_member() {\n        assert_eq!(super::member_fn(), 1);\n    }\n}\n",
    )
    .unwrap();
    fs::write(
        nested.join("Cargo.toml"),
        "[package]\nname = \"nested\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .unwrap();
    fs::write(
        nested.join("src").join("lib.rs"),
        "pub fn nested_fn() -> u32 { 2 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn covers_nested() {\n        assert_eq!(super::nested_fn(), 2);\n    }\n}\n",
    )
    .unwrap();
    fs::write(
        root.join(".kissconfig"),
        "[gate]\n\
         test_coverage_threshold = 75\n\
         test_coverage_scope = \"by_file\"\n\
         duplication_enabled = false\n\
         orphan_module_enabled = false\n",
    )
    .unwrap();
    let mut git = kiss::scrubbed_git_command(root);
    assert!(git.arg("init").status().unwrap().success());
}

#[test]
fn nested_non_member_path_target_fails_fast() {
    let tmp = TempDir::new().unwrap();
    write_nested_workspace_repo(tmp.path());
    let output = kiss_binary()
        .current_dir(tmp.path())
        .args(["test", "nested", "--force"])
        .output()
        .expect("run kiss test nested");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        stderr.contains("nested Cargo crate") && stderr.contains("nested"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn nested_non_member_sources_do_not_fail_by_file_gate() {
    let tmp = TempDir::new().unwrap();
    write_nested_workspace_repo(tmp.path());
    let output = kiss_binary()
        .current_dir(tmp.path())
        .args(["test", ".", "--force"])
        .output()
        .expect("run kiss test .");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("nested/src/lib.rs"),
        "nested crate must not appear in by_file findings.\nstdout:\n{stdout}"
    );
    assert!(
        stderr.contains("skipping coverage scoring for nested non-member"),
        "expected nested-crate skip warning.\nstderr:\n{stderr}"
    );
}
