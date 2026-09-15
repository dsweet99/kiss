use crate::common::seed_rust_runtime_coverage;
use crate::support::git::git_command;
use serde_json::Value;
use std::fs;
use tempfile::TempDir;

#[test]
fn rust_runtime_coverage_seed_publishes_covering_selector() {
    let repo = TempDir::new().unwrap();
    init_git_repo(repo.path());
    write_incremental_rust_repo(&repo, "1");
    seed_incremental_coverage(repo.path());
    assert_eq!(
        passed_selector_entry_count(repo.path()),
        1,
        "seed should publish the covering selector"
    );
}

#[test]
fn rust_runtime_coverage_reseed_after_edit_publishes_covering_selector() {
    let repo = TempDir::new().unwrap();
    init_git_repo(repo.path());
    write_incremental_rust_repo(&repo, "2");
    seed_incremental_coverage(repo.path());
    assert_eq!(
        passed_selector_entry_count(repo.path()),
        1,
        "editing the package should still publish the covering selector"
    );
}

fn seed_incremental_coverage(repo: &std::path::Path) {
    seed_rust_runtime_coverage(
        repo,
        &[(
            "covers_lib_first",
            vec![("covered/src/lib.rs", vec![1])],
        )],
    );
}

fn init_git_repo(repo: &std::path::Path) {
    assert!(git_command(repo).args(["init"]).status().unwrap().success());
    for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
        assert!(git_command(repo)
            .args(["config", kv.0, kv.1])
            .status()
            .unwrap()
            .success());
    }
    assert!(git_command(repo)
        .args(["commit", "--allow-empty", "-m", "init"])
        .status()
        .unwrap()
        .success());
}

fn passed_selector_entry_count(repo: &std::path::Path) -> usize {
    let entries = repo.join(".kiss/rust_llvm_cov_cache/entries");
    let mut selectors = std::collections::BTreeSet::new();
    let Ok(rd) = fs::read_dir(entries) else {
        return 0;
    };
    for entry in rd.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        if raw["status"] == "Passed"
            && let Some(sel) = raw["selector"].as_str()
        {
            selectors.insert(sel.to_string());
        }
    }
    selectors.len()
}

fn write_incremental_rust_repo(repo: &TempDir, value: &str) {
    let covered = repo.path().join("covered");
    fs::create_dir_all(covered.join("src")).unwrap();
    fs::create_dir_all(covered.join("tests")).unwrap();
    fs::write(
        repo.path().join(".kissconfig"),
        "[global]\nduplication_enabled = false\n[test]\ntest_coverage_threshold = 100\n[python]\n[rust]\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"covered\"]\nresolver = \"3\"\n",
    )
    .unwrap();
    fs::write(
        covered.join("Cargo.toml"),
        "[package]\nname = \"covered\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        covered.join("src").join("lib.rs"),
        format!("pub fn value() -> i32 {{ {value} }}\n"),
    )
    .unwrap();
    fs::write(
        covered.join("tests").join("covers_lib.rs"),
        format!(
            "use covered::value;\n\n\
             #[test]\n\
             fn covers_lib_first() {{\n\
                 assert_eq!(value(), {value});\n\
             }}\n"
        ),
    )
    .unwrap();
}
