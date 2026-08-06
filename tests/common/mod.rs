#![allow(dead_code)]

use kiss::parsing::{ParsedFile, create_parser, parse_file};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tree_sitter::Node;

mod python_seed_helpers;
use python_seed_helpers::{
    python_entries_fingerprint, python_rslip_cache_root_for_repo, python_source_input_fingerprint,
};

/// Repo-local `.kiss` cache root (used by integration tests that assert
/// where `check_full_*.bin` lands after `kiss check` / `kiss stats`).
pub fn cache_dir_under(repo: &Path) -> PathBuf {
    repo.join(".kiss")
}

/// True for files matching `check_full_*.bin` (the full-check analyze
/// cache file). Used as the predicate for [`list_full_check_cache_files`].
pub fn is_full_check_cache_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    name.starts_with("check_full_")
        && Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("bin"))
}

/// Sorted list of `check_full_*.bin` files in `repo/.kiss`.
/// Returns an empty `Vec` if the cache dir does not exist yet.
pub fn list_full_check_cache_files(repo: &Path) -> Vec<PathBuf> {
    let dir = cache_dir_under(repo);
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<_> = rd
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| is_full_check_cache_file(p))
        .collect();
    out.sort();
    out
}

pub fn generate_lockfile(repo: &Path) {
    let lockfile = Command::new("cargo")
        .arg("generate-lockfile")
        .current_dir(repo)
        .output()
        .expect("cargo generate-lockfile should run");
    assert!(
        lockfile.status.success(),
        "cargo generate-lockfile failed: {}",
        String::from_utf8_lossy(&lockfile.stderr)
    );
}

pub type PythonRuntimeCoverageSeed<'a> = (&'a str, Vec<(&'a str, Vec<u32>)>);
pub type RustRuntimeCoverageSeed<'a> = (&'a str, Vec<(&'a str, Vec<u32>)>);

pub fn seed_python_runtime_coverage(repo: &Path, entries: &[PythonRuntimeCoverageSeed<'_>]) {
    let repo = repo.canonicalize().unwrap();
    let cache_root = python_rslip_cache_root_for_repo(&repo);
    fs::create_dir_all(cache_root.join("entries")).unwrap();
    let python_version = python_command_output(
        &repo,
        &[
            "-c",
            "import sys; print('.'.join(map(str, sys.version_info[:3])))",
        ],
    );
    let pytest_version =
        python_command_output(&repo, &["-c", "import pytest; print(pytest.__version__)"]);
    let env = relevant_python_env(&repo);
    let env_json = env
        .iter()
        .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
        .collect::<serde_json::Map<_, _>>();
    let mut selectors = Vec::new();
    for (selector, coverage_files) in entries {
        selectors.push((*selector).to_string());
        let req = rslip::RslipRequest {
            nodeid: (*selector).to_string(),
            cwd: repo.clone(),
            source_root: repo.clone(),
            python: PathBuf::from("python"),
            python_version: python_version.clone(),
            pytest_version: pytest_version.clone(),
            pytest_args: Vec::new(),
            env: env.clone(),
            cache_root: cache_root.clone(),
            force_rerun: false,
            timeout: None,
        };
        let fingerprint = rslip::cache_fingerprint_for_request(&req).unwrap();
        let files = coverage_files
            .iter()
            .map(|(file, lines)| {
                (
                    coverage_seed_file(repo.as_path(), file),
                    serde_json::json!(lines),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let payload = serde_json::json!({
            "schema_version": rslip::CACHE_SCHEMA_VERSION,
            "nodeid": selector,
            "status": "Passed",
            "exit_code": 0,
            "duration": { "secs": 0, "nanos": 1_000_000 },
            "coverage": { "files": files },
        });
        fs::write(
            cache_root
                .join("entries")
                .join(format!("{fingerprint}.json")),
            format!("{}\n", serde_json::to_string(&payload).unwrap()),
        )
        .unwrap();
    }
    selectors.sort();
    selectors.dedup();
    let manifest = serde_json::json!({
        "schema_version": "rslip-python-population-v1",
        "cache_schema_version": rslip::CACHE_SCHEMA_VERSION,
        "source_root": repo.to_string_lossy().to_string(),
        "selector_discovery_version": "python-selector-discovery-v2",
        "python_version": python_version,
        "pytest_version": pytest_version,
        "pytest_args": [],
        "env": env_json,
        "input_fingerprint": python_source_input_fingerprint(&repo),
        "entries_fingerprint": python_entries_fingerprint(&cache_root),
        "selectors": selectors,
    });
    fs::write(
        cache_root.join("population.json"),
        format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap()),
    )
    .unwrap();
}

pub fn seed_rust_runtime_coverage(repo: &Path, entries: &[RustRuntimeCoverageSeed<'_>]) {
    let repo = repo.canonicalize().unwrap();
    let selectors = sorted_unique_selectors(entries.iter().map(|(selector, _)| *selector));
    let req = rust_runtime_coverage_request(&repo, &selectors);
    let tools = rust_runtime_coverage_tool_identity(&repo);
    let identity = rust_llvm_cov_runner::batch_identity(&req, &tools).unwrap();
    for (selector, coverage_files) in entries {
        let fingerprint =
            rust_llvm_cov_runner::entry_fingerprint(&identity.input_digest, &req, &tools, selector);
        let files = coverage_files
            .iter()
            .map(|(file, lines)| {
                (
                    coverage_seed_file(repo.as_path(), file),
                    lines.iter().copied().collect::<BTreeSet<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let outcome = rust_llvm_cov_runner::RustLlvmCovOutcome {
            selector: (*selector).to_string(),
            status: rpytest_runner::TestStatus::Passed,
            exit_code: Some(0),
            duration: std::time::Duration::from_millis(1),
            coverage: rust_llvm_cov_runner::RustLineCoverage { files },
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: rust_llvm_cov_runner::RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        };
        let entry = rust_llvm_cov_runner::RustCovCacheEntry::from_outcome(
            &outcome,
            &identity.generation_fingerprint,
        );
        rust_llvm_cov_runner::store_rust_cov_cache_entry(&req.cache_root, &fingerprint, &entry)
            .unwrap();
    }
    rust_llvm_cov_runner::publish_derived_state(&req, &tools, &identity, &selectors, false)
        .unwrap();
}

fn rust_runtime_coverage_request(
    repo: &Path,
    selectors: &[String],
) -> rust_llvm_cov_runner::RustCoverageBatchRequest {
    let (delegated_runners, runner_map_fingerprint, host_platform) =
        rust_llvm_cov_runner::placeholder_delegated_runner_fields();
    rust_llvm_cov_runner::RustCoverageBatchRequest {
        cwd: repo.to_path_buf(),
        source_root: repo.to_path_buf(),
        cargo: PathBuf::from("cargo"),
        cache_root: repo.join(".kiss").join("rust_llvm_cov_cache"),
        logical_selectors: selectors.to_vec(),
        cargo_args: vec!["--workspace".to_string()],
        test_args: Vec::new(),
        env: relevant_rust_env(),
        force_rerun: false,
        jobs: 1,
        generated_config: repo
            .join(".kiss")
            .join("rust_llvm_cov_cache")
            .join("runs")
            .join("test-seed")
            .join("nextest.toml"),
        population_publication_selectors: Some(selectors.to_vec()),
        delegated_runners,
        runner_map_fingerprint,
        host_platform,
        coverage_output_mode: rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
    }
}

fn rust_runtime_coverage_tool_identity(
    repo: &Path,
) -> rust_llvm_cov_runner::RustCoverageToolIdentity {
    rust_llvm_cov_runner::RustCoverageToolIdentity {
        cargo_version: command_output(repo, "cargo", &["--version"]),
        llvm_cov_version: command_output(repo, "cargo", &["llvm-cov", "--version"]),
        rustc_version: command_output(repo, "rustc", &["-Vv"]),
        cargo_nextest_version: command_output(repo, "cargo", &["nextest", "--version"]),
    }
}

fn relevant_rust_env() -> BTreeMap<String, String> {
    kiss::env_map_from_allowlist(&[
        "RUSTFLAGS",
        "RUSTDOCFLAGS",
        "CARGO_TARGET_DIR",
        "LLVM_PROFILE_FILE",
    ])
}

fn command_output(repo: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{program} command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn sorted_unique_selectors<'a>(selectors: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut selectors = selectors.map(str::to_string).collect::<Vec<_>>();
    selectors.sort();
    selectors.dedup();
    selectors
}

fn coverage_seed_file(repo: &Path, file: &str) -> String {
    let path = Path::new(file);
    if file.starts_with('<') || path.is_absolute() || file.starts_with(".kiss/") {
        file.to_string()
    } else {
        repo.join(path).to_string_lossy().to_string()
    }
}

fn python_command_output(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("python")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "python command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn relevant_python_env(repo: &Path) -> BTreeMap<String, String> {
    kiss::python_coverage_env_map(repo)
}

pub fn parse_python_source(code: &str) -> ParsedFile {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "{code}").unwrap();
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, tmp.path()).expect("should parse temp source")
}

pub fn first_function_node(p: &ParsedFile) -> Node<'_> {
    let root = p.tree.root_node();
    for i in 0..root.child_count() {
        if let Some(node) = root.child(i)
            && node.kind() == "function_definition"
        {
            return node;
        }
    }

    for i in 0..root.child_count() {
        if let Some(node) = root.child(i)
            && node.kind() == "decorated_definition"
        {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "function_definition" {
                    return child;
                }
            }
        }
    }

    panic!("function_definition");
}

pub fn first_function_or_async_node(p: &ParsedFile) -> Node<'_> {
    let root = p.tree.root_node();
    (0..root.child_count())
        .filter_map(|i| root.child(i))
        .find(|n| n.kind() == "function_definition" || n.kind() == "async_function_definition")
        .expect("function_definition")
}
