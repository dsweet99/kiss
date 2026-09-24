//! In-process e2e: `kiss test --retry-bad` keeps prior PASS cached and reruns FAIL.
#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::bin_cli::args::TestInvocation;
use crate::cwd_test_lock;
use crate::test_runner::RunTestCmdArgs;
use crate::test_runner::capture_stdout::capture_stdout;
use crate::test_runner::run_test;
use crate::test_runner::workspace_selector_cache::store_python_workspace_selectors;

fn init_git(root: &Path) {
    assert!(
        kiss::scrubbed_git_command(root)
            .arg("init")
            .status()
            .unwrap()
            .success()
    );
}

fn write_fixture(root: &Path) {
    fs::write(
        root.join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         [test]\n\
         test_coverage_threshold = 0\n\
         orphan_detection = false\n\
         num_jobs = 1\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();
    fs::write(root.join("lib.py"), "VALUE = 1\n").unwrap();
    fs::write(
        root.join("test_lib.py"),
        "import lib\n\n\ndef test_ok():\n    assert True\n\n\ndef test_flip():\n    assert lib.VALUE == 1\n",
    )
    .unwrap();
}

fn python_versions(root: &Path) -> (String, String) {
    let py = std::process::Command::new("python")
        .args([
            "-c",
            "import sys; print('.'.join(map(str, sys.version_info[:3])))",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    let pytest = std::process::Command::new("python")
        .args(["-c", "import pytest; print(pytest.__version__)"])
        .current_dir(root)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&py.stdout).trim().to_string(),
        String::from_utf8_lossy(&pytest.stdout).trim().to_string(),
    )
}

fn seed_cache(
    root: &Path,
    versions: &(String, String),
    selector: &str,
    coverage_file: &str,
    lines: &[u32],
    status: &str,
    exit_code: i32,
) {
    let root = root.canonicalize().unwrap();
    let (python_version, pytest_version) = versions;
    let env = kiss::python_coverage_env_map(&root);
    let cache_root =
        crate::test_runner::python_coverage_index::storage::python_coverage_cache_root(&root)
            .unwrap();
    fs::create_dir_all(cache_root.join("entries")).unwrap();
    let req = kiss::rslip::RslipRequest {
        nodeid: selector.to_string(),
        cwd: root.clone(),
        source_root: root.clone(),
        python: PathBuf::from("python"),
        python_version: python_version.clone(),
        pytest_version: pytest_version.clone(),
        pytest_args: Vec::new(),
        env: env.clone(),
        cache_root: cache_root.clone(),
        force_rerun: false,
        timeout: None,
        content_fingerprint: None,
    };
    let fingerprint = kiss::rslip::cache_fingerprint_for_request(&req).unwrap();
    let abs = root.join(coverage_file).to_string_lossy().to_string();
    let files = BTreeMap::from([(abs.clone(), lines.iter().copied().collect::<BTreeSet<_>>())]);
    let coverage = kiss::rslip::LineCoverage {
        files: files.clone(),
    };
    let covered_digests =
        kiss::rslip::covered_file_digests_for(&root, selector, &coverage).unwrap_or_default();
    let files_json = files
        .iter()
        .map(|(file, lines)| (file.clone(), serde_json::json!(lines)))
        .collect::<serde_json::Map<_, _>>();
    let payload = serde_json::json!({
        "schema_version": kiss::rslip::CACHE_SCHEMA_VERSION,
        "nodeid": selector,
        "status": status,
        "exit_code": exit_code,
        "duration": { "secs": 0, "nanos": 1_000_000 },
        "coverage": { "files": files_json },
        "covered_digests": covered_digests,
    });
    fs::write(
        cache_root
            .join("entries")
            .join(format!("{fingerprint}.json")),
        format!("{}\n", serde_json::to_string(&payload).unwrap()),
    )
    .unwrap();
}

fn retry_bad_args() -> RunTestCmdArgs<'static> {
    let gate = kiss::GateConfig {
        test_coverage_threshold: 0,
        orphan_detection: false,
        max_unit_test_seconds: Vec::new(),
        ..Default::default()
    };
    RunTestCmdArgs {
        invocation: TestInvocation::Targets(vec![
            "test_lib.py::test_ok".into(),
            "test_lib.py::test_flip".into(),
        ]),
        target_request: crate::test_runner::target_request::operands_request(
            &[
                "test_lib.py::test_ok".into(),
                "test_lib.py::test_flip".into(),
            ],
            Some(kiss::Language::Python),
            &[],
        ),
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: false,
        force_rerun: false,
        force_bad: true,
        metrics: false,
        coverage_all: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: Some(kiss::Language::Python),
        config_main_branch: None,
        gate_config: gate,
    }
}

#[test]
fn retry_bad_keeps_prior_pass_cached_and_reruns_fail() {
    let _cwd = cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init_git(tmp.path());
    write_fixture(tmp.path());
    assert!(store_python_workspace_selectors(
        tmp.path(),
        &[],
        &[
            "test_lib.py::test_ok".into(),
            "test_lib.py::test_flip".into(),
        ],
        &[],
    ));
    let versions = python_versions(tmp.path());
    seed_cache(
        tmp.path(),
        &versions,
        "test_lib.py::test_ok",
        "test_lib.py",
        &[4, 5],
        "Passed",
        0,
    );
    seed_cache(
        tmp.path(),
        &versions,
        "test_lib.py::test_flip",
        "lib.py",
        &[1],
        "Failed",
        1,
    );

    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let mut code = 1;
    let out = capture_stdout(|| {
        code = run_test(retry_bad_args());
    });
    std::env::set_current_dir(orig).unwrap();
    assert_eq!(code, 0, "retry-bad must exit 0; out={out}");

    assert!(
        out.contains("kiss test: rslip prepared hits=1 misses=1")
            && out.contains("PASS test_lib.py::test_ok"),
        "prior PASS must stay cached; out={out}"
    );
    assert!(
        out.contains("PASS:") && out.contains("test_flip"),
        "prior FAIL must rerun; out={out}"
    );
}
