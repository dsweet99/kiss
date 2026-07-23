use crate::common::generate_lockfile;
use crate::support::git::git_command;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn rust_runtime_coverage_refresh_publishes_incremental_generation() {
    let home = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    init_git_repo(repo.path());
    write_incremental_rust_repo(&repo, "1");
    generate_lockfile(repo.path());

    let (cold_binaries, cold_maps) = assert_cold_aggregate_matches_selector_entries(&home, &repo);

    write_incremental_rust_repo(&repo, "2");
    assert_incremental_repair_replaces_only_covered_binary(&home, &repo, cold_binaries, &cold_maps);
    assert_warm_check_reuses_current_aggregate(&home, &repo);
}

fn assert_cold_aggregate_matches_selector_entries(
    home: &TempDir,
    repo: &TempDir,
) -> (usize, BTreeMap<String, Value>) {
    let cold = run_kiss_cov_rust(home, repo);
    assert_success("cold kiss cov", &cold);
    let cold_stderr = String::from_utf8_lossy(&cold.stderr);
    let (cold_binaries, cold_exports) = parse_aggregate_refresh_counts(&cold_stderr)
        .unwrap_or_else(|| panic!("missing cold aggregate Rust refresh line:\n{cold_stderr}"));
    assert_eq!(cold_binaries, 2, "fixture should publish two test binaries");
    assert!(
        cold_exports < 4,
        "aggregate exports should be fewer than the four test instances.\nstderr:\n{cold_stderr}"
    );
    let cold_maps = aggregate_line_maps(repo.path());
    let selector_entries = run_kiss_test_rust_force(home, repo);
    assert_success("forced selector-entry kiss test", &selector_entries);
    assert_eq!(
        ordinary_source_covered_lines(aggregate_covered_lines(repo.path())),
        selector_entry_covered_lines(repo.path()),
        "check aggregate physical-line coverage should match selector-entry reference coverage"
    );
    (cold_binaries, cold_maps)
}

fn assert_incremental_repair_replaces_only_covered_binary(
    home: &TempDir,
    repo: &TempDir,
    cold_binaries: usize,
    cold_maps: &BTreeMap<String, Value>,
) {
    let incremental = run_kiss_cov_rust(home, repo);
    assert_success("incremental kiss cov", &incremental);
    let incremental_stderr = String::from_utf8_lossy(&incremental.stderr);
    let (binaries, exports) = parse_aggregate_refresh_counts(&incremental_stderr)
        .unwrap_or_else(|| panic!("missing aggregate Rust refresh line:\n{incremental_stderr}"));
    assert_eq!(binaries, cold_binaries);
    assert_eq!(
        exports, 1,
        "editing only the covered package should export only its replacement binary.\nstderr:\n{incremental_stderr}"
    );
    assert!(
        binaries > 0 && exports > 0 && exports <= 2,
        "ordinary source edit should publish aggregate Rust coverage by binary.\nstderr:\n{incremental_stderr}"
    );
    let incremental_maps = aggregate_line_maps(repo.path());
    let stable_id = cold_maps
        .iter()
        .find(|(_, line_map)| line_map.get("stable/src/lib.rs").is_some())
        .map(|(binary_id, _)| binary_id.clone())
        .expect("stable binary map should be present");
    assert_eq!(
        incremental_maps.get(&stable_id),
        cold_maps.get(&stable_id),
        "incremental repair should retain the unchanged stable binary's exact stored map.\ncold={cold_maps:#?}\nincremental={incremental_maps:#?}"
    );
}

fn assert_warm_check_reuses_current_aggregate(home: &TempDir, repo: &TempDir) {
    let warm = run_kiss_cov_rust(home, repo);
    assert_success("warm kiss cov", &warm);
    let warm_stderr = String::from_utf8_lossy(&warm.stderr);
    assert!(
        !warm_stderr.contains("refreshing Rust runtime coverage"),
        "warm check should validate the merged generation without refreshing.\nstderr:\n{warm_stderr}"
    );
}

fn parse_aggregate_refresh_counts(stderr: &str) -> Option<(usize, usize)> {
    let prefix = "kiss cov: refreshed Rust runtime coverage ";
    let line = stderr.lines().find(|line| line.starts_with(prefix))?;
    let fields = line.strip_prefix(prefix)?;
    let mut binaries = None;
    let mut exports = None;
    for field in fields.split_whitespace() {
        if let Some(value) = field.strip_prefix("rust_aggregate_binaries=") {
            binaries = value.parse().ok();
        }
        if let Some(value) = field.strip_prefix("rust_aggregate_exports=") {
            exports = value.parse().ok();
        }
    }
    Some((binaries?, exports?))
}

fn aggregate_line_maps(repo: &std::path::Path) -> BTreeMap<String, Value> {
    let path = repo.join(".kiss/rust_llvm_cov_cache/check_aggregate.json");
    let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    raw["binaries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| {
            (
                record["id"].as_str().unwrap().to_string(),
                record["line_map"].clone(),
            )
        })
        .collect()
}

fn init_git_repo(repo: &std::path::Path) {
    assert!(git_command(repo).args(["init"]).status().unwrap().success());
    for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
        assert!(
            git_command(repo)
                .args(["config", kv.0, kv.1])
                .status()
                .unwrap()
                .success()
        );
    }
    assert!(
        git_command(repo)
            .args(["commit", "--allow-empty", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
}

fn aggregate_covered_lines(repo: &std::path::Path) -> BTreeMap<String, BTreeSet<u32>> {
    let path = repo.join(".kiss/rust_llvm_cov_cache/check_aggregate.json");
    let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    line_map_from_value(&raw["aggregate_covered_lines"])
}

fn selector_entry_covered_lines(repo: &std::path::Path) -> BTreeMap<String, BTreeSet<u32>> {
    let entries = repo.join(".kiss/rust_llvm_cov_cache/entries");
    let mut covered = BTreeMap::<String, BTreeSet<u32>>::new();
    for entry in fs::read_dir(entries).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(raw["status"], "Passed");
        for (file, lines) in line_map_from_value(&raw["coverage"]["files"]) {
            let relative = file
                .strip_prefix(&format!("{}/", repo.display()))
                .unwrap_or(&file)
                .to_string();
            covered.entry(relative).or_default().extend(lines);
        }
    }
    covered
}

fn ordinary_source_covered_lines(
    covered: BTreeMap<String, BTreeSet<u32>>,
) -> BTreeMap<String, BTreeSet<u32>> {
    covered
        .into_iter()
        .filter(|(file, _)| file == "covered/src/lib.rs" || file == "stable/src/lib.rs")
        .collect()
}

fn line_map_from_value(value: &Value) -> BTreeMap<String, BTreeSet<u32>> {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(file, lines)| {
            (
                file.clone(),
                lines
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|line| line.as_u64().unwrap() as u32)
                    .collect(),
            )
        })
        .collect()
}

fn write_incremental_rust_repo(repo: &TempDir, value: &str) {
    let covered = repo.path().join("covered");
    let stable = repo.path().join("stable");
    fs::create_dir_all(covered.join("src")).unwrap();
    fs::create_dir_all(covered.join("tests")).unwrap();
    fs::create_dir_all(stable.join("src")).unwrap();
    fs::create_dir_all(stable.join("tests")).unwrap();
    fs::write(
        repo.path().join(".kissconfig"),
        "[gate]\n\
         test_coverage_threshold = 100\n\
         duplication_enabled = false\n\
         orphan_module_enabled = false\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[workspace]\n\
         members = [\"covered\", \"stable\"]\n\
         resolver = \"3\"\n",
    )
    .unwrap();
    write_covered_package(&covered, value);
    write_stable_package(&stable);
}

fn write_covered_package(covered: &std::path::Path, value: &str) {
    fs::write(
        covered.join("Cargo.toml"),
        "[package]\n\
         name = \"covered\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n",
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
             }}\n\n\
             #[test]\n\
             fn covers_lib_second() {{\n\
                 assert_eq!(value(), {value});\n\
             }}\n"
        ),
    )
    .unwrap();
}

fn write_stable_package(stable: &std::path::Path) {
    fs::write(
        stable.join("Cargo.toml"),
        "[package]\n\
         name = \"stable\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        stable.join("src").join("lib.rs"),
        "pub fn stable_value() -> i32 { 10 }\n",
    )
    .unwrap();
    fs::write(
        stable.join("tests").join("covers_stable.rs"),
        "use stable::stable_value;\n\n\
         #[test]\n\
         fn covers_stable_first() {\n\
             assert_eq!(stable_value(), 10);\n\
         }\n\n\
         #[test]\n\
         fn covers_stable_second() {\n\
             assert_eq!(stable_value(), 10);\n\
         }\n",
    )
    .unwrap();
}

fn run_kiss_cov_rust(home: &TempDir, repo: &TempDir) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("cov")
        .arg("--lang")
        .arg("rust")
        .arg(repo.path())
        .current_dir(repo.path())
        .env("HOME", home.path())
        .env_remove("LLVM_PROFILE_FILE")
        .output()
        .expect("kiss cov should run")
}

fn run_kiss_test_rust_force(home: &TempDir, repo: &TempDir) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kiss"))
        .arg("--lang")
        .arg("rust")
        .arg("test")
        .arg("commit")
        .arg("--force")
        .current_dir(repo.path())
        .env("HOME", home.path())
        .env_remove("LLVM_PROFILE_FILE")
        .output()
        .expect("kiss test should run")
}

fn assert_success(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
