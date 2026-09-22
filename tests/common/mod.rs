#![allow(dead_code)]

use kiss::parsing::{ParsedFile, create_parser, parse_file};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use tree_sitter::Node;

mod python_seed_helpers;
use python_seed_helpers::{
    python_entries_fingerprint, python_rslip_cache_root_for_repo, python_seeded_population_is_current,
    python_source_input_fingerprint,
};

pub fn is_cli_wall_timing_line(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("kiss: ") else {
        return false;
    };
    rest.ends_with("ms") || (rest.ends_with('s') && rest.contains('.'))
}

pub fn without_cli_wall_timing(s: &str) -> String {
    s.lines()
        .filter(|line| !is_cli_wall_timing_line(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Prefer tmpfs for TempDir-backed publish barriers (ext4 /tmp fsync is slow).
pub fn prefer_tmpfs_tmpdir() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if std::env::var_os("TMPDIR").is_none() && Path::new("/dev/shm").is_dir() {
            unsafe { std::env::set_var("TMPDIR", "/dev/shm") };
        }
    });
}

fn force_tmpfs_on_load() {
    prefer_tmpfs_tmpdir();
}

/// Touch prefer_tmpfs as soon as common is first used.
static FORCE_TMPFS: Once = Once::new();
fn ensure_tmpfs() {
    FORCE_TMPFS.call_once(force_tmpfs_on_load);
}

/// After seeding runtime coverage, apply a repo change and assert the population
/// identity no longer matches (equivalent to `load_python_runtime_coverage` failing
/// closed with a stale/missing population rather than reusing the seed).
pub fn assert_seeded_python_runtime_coverage_stale_after(
    repo: &Path,
    apply_change: impl FnOnce(),
) {
    assert!(
        python_seeded_population_is_current(repo),
        "seeded population must be current before the source edit"
    );
    let recorded_input = {
        let cache_root = python_rslip_cache_root_for_repo(&repo.canonicalize().unwrap());
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(cache_root.join("population.json")).expect("seeded population manifest"),
        )
        .expect("population manifest json");
        manifest["input_fingerprint"]
            .as_str()
            .expect("input_fingerprint")
            .to_string()
    };
    apply_change();
    let current_input = python_source_input_fingerprint(repo);
    assert_ne!(
        recorded_input, current_input,
        "source edit must drift the workspace input fingerprint"
    );
    assert!(
        !python_seeded_population_is_current(repo),
        "stale seed must not be treated as a current/reusable population after source change"
    );
}

pub fn cache_dir_under(repo: &Path) -> PathBuf {
    repo.join(".kiss")
}

pub const BUILTIN_LANGUAGE_CONFIG: &str = "[python]\n[rust]\n";

pub fn write_builtin_language_config(dir: &Path) -> PathBuf {
    let path = dir.join("builtin.kissconfig");
    fs::write(&path, BUILTIN_LANGUAGE_CONFIG).unwrap();
    path
}

pub fn is_full_check_cache_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    name.starts_with("check_full_")
        && Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("bin"))
}

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

/// Strip parent `kiss test` / llvm-cov env so nested cargo work is not inflated under suite load.
pub fn scrub_parent_coverage_env(cmd: &mut Command) {
    const KEYS: &[&str] = &[
        "LLVM_PROFILE_FILE",
        "LLVM_PROFILE_FILE_NAME",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTDOCFLAGS",
        "CARGO_TARGET_DIR",
        "CARGO_LLVM_COV_TARGET_DIR",
        "CARGO_LLVM_COV_BUILD_DIR",
        "KISS_COVERAGE_RUNTIME_REFRESH_ACTIVE",
        "KISS_RUST_COVERAGE_PROFILE_POOL",
    ];
    for key in KEYS {
        cmd.env_remove(key);
    }
    // Cap nested cargo parallelism so suite-load contention is less likely to breach the 60s SLA,
    // while still allowing a little parallelism so fixture compiles finish under 2s.
    cmd.env("CARGO_BUILD_JOBS", "2");
}

/// Keep rustup/cargo usable when a test overrides `HOME` to a TempDir.
pub fn preserve_toolchain_homes(cmd: &mut Command) {
    if std::env::var_os("RUSTUP_HOME").is_none()
        && let Some(home) = std::env::var_os("HOME")
    {
        cmd.env("RUSTUP_HOME", PathBuf::from(home).join(".rustup"));
    }
    if std::env::var_os("CARGO_HOME").is_none()
        && let Some(home) = std::env::var_os("HOME")
    {
        cmd.env("CARGO_HOME", PathBuf::from(home).join(".cargo"));
    }
}

pub type PythonRuntimeCoverageSeed<'a> = (&'a str, Vec<(&'a str, Vec<u32>)>);
pub type RustRuntimeCoverageSeed<'a> = (&'a str, Vec<(&'a str, Vec<u32>)>);

pub fn persistent_cargo_target(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("kiss-test-targets").join(name);
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn persistent_rust_failure_repo() -> PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let root = std::env::temp_dir().join("kiss-rust-failure-fixture");
        if root.join("Cargo.toml").is_file() {
            return root;
        }
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join(".kissconfig"),
            "[global]\nduplication_enabled = false\n[test]\ntest_coverage_threshold = 0\norphan_detection = false\n[python]\n[rust]\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("src").join("lib.rs"),
            "pub fn value() -> u32 { 1 }\n\
#[cfg(test)]\n\
mod tests {\n\
    #[test]\n\
    fn gets_value() {\n\
        assert_eq!(super::value(), 2);\n\
    }\n\
}\n",
        )
        .unwrap();
        let init = kiss::scrubbed_git_command(&root)
            .args(["init", "-q"])
            .output()
            .expect("git init");
        assert!(init.status.success(), "git init failed");
        let add = kiss::scrubbed_git_command(&root)
            .args(["add", "."])
            .output()
            .expect("git add");
        assert!(add.status.success(), "git add failed");
        let commit = kiss::scrubbed_git_command(&root)
            .args(["commit", "-q", "-m", "init"])
            .env("GIT_AUTHOR_NAME", "kiss")
            .env("GIT_AUTHOR_EMAIL", "kiss@test")
            .env("GIT_COMMITTER_NAME", "kiss")
            .env("GIT_COMMITTER_EMAIL", "kiss@test")
            .output()
            .expect("git commit files");
        assert!(commit.status.success(), "git commit files failed");
        let target_dir = persistent_cargo_target("rust-failure-demo");
        let status = Command::new("cargo")
            .args(["test", "--no-run", "-q"])
            .current_dir(&root)
            .env("CARGO_TARGET_DIR", &target_dir)
            .status()
            .expect("prebuild rust failure fixture");
        assert!(status.success(), "rust failure fixture prebuild failed");
        root
    })
    .clone()
}

pub fn persistent_python_failure_repo() -> PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let root = std::env::temp_dir().join("kiss-python-failure-fixture");
        if root.join("test_lib.py").is_file() {
            return root;
        }
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(".kissconfig"),
            "[global]\nduplication_enabled = false\n[test]\ntest_coverage_threshold = 0\norphan_detection = false\n[python]\n[rust]\n",
        )
        .unwrap();
        fs::write(root.join("lib.py"), "def f():\n    return 0\n").unwrap();
        fs::write(
            root.join("test_lib.py"),
            "from lib import f\n\ndef test_f():\n    assert f() == 1\n",
        )
        .unwrap();
        let init = kiss::scrubbed_git_command(&root)
            .args(["init", "-q"])
            .output()
            .expect("git init");
        assert!(init.status.success(), "git init failed");
        let add = kiss::scrubbed_git_command(&root)
            .args(["add", "."])
            .output()
            .expect("git add");
        assert!(add.status.success(), "git add failed");
        let commit = kiss::scrubbed_git_command(&root)
            .args(["commit", "-q", "-m", "init"])
            .env("GIT_AUTHOR_NAME", "kiss")
            .env("GIT_AUTHOR_EMAIL", "kiss@test")
            .env("GIT_COMMITTER_NAME", "kiss")
            .env("GIT_COMMITTER_EMAIL", "kiss@test")
            .output()
            .expect("git commit files");
        assert!(commit.status.success(), "git commit files failed");
        root
    })
    .clone()
}

pub fn copy_repo_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("copy destination");
    let status = Command::new("cp")
        .args(["-a", &format!("{}/.", src.display()), &format!("{}/", dst.display())])
        .status()
        .expect("copy_repo_tree");
    assert!(status.success(), "copy_repo_tree failed");
    retarget_cloned_kiss_cache(src, dst);
}

fn retarget_cloned_kiss_cache(src: &Path, dst: &Path) {
    let old_root = src.canonicalize().unwrap_or_else(|_| src.to_path_buf());
    let new_root = dst.canonicalize().unwrap_or_else(|_| dst.to_path_buf());
    if old_root == new_root {
        return;
    }
    let old = old_root.to_string_lossy();
    let new = new_root.to_string_lossy();
    let kiss = new_root.join(".kiss");
    if !kiss.is_dir() {
        return;
    }
    let mut stack = vec![kiss];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let is_text = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json") || ext == "toml");
            if !is_text {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if !text.contains(old.as_ref()) {
                continue;
            }
            fs::write(&path, text.replace(old.as_ref(), new.as_ref())).expect("retarget cache");
        }
    }
}

pub fn persistent_python_coverage_gap_repo() -> PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        // Keep the path short: Unix sockets under .kiss/watch must fit sun_path (~108).
        let root = std::env::temp_dir().join("kiss-pcg");
        let stamp = root.join(".kiss").join("fixture_inplace_ok");
        if root.join(".git").join("HEAD").is_file()
            && root.join(".kiss").is_dir()
            && root.join("lib.py").is_file()
            && stamp.is_file()
            && fs::read_to_string(&stamp).ok().as_deref() == Some(root.to_string_lossy().as_ref())
        {
            return root;
        }
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("coverage-gap fixture root");
        let init = kiss::scrubbed_git_command(&root)
            .args(["init", "-q", "-b", "main"])
            .output()
            .expect("git init");
        assert!(init.status.success(), "git init failed");
        for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
            kiss::scrubbed_git_command(&root)
            .args(["config", kv.0, kv.1])
                .status()
                .expect("git config");
        }
        fs::write(
            root.join("lib.py"),
            "def f():\n    return 0\ndef unused():\n    return 1\n",
        )
        .unwrap();
        fs::write(
            root.join("test_lib.py"),
            "from lib import f\n\ndef test_f():\n    assert f() == 0\n",
        )
        .unwrap();
        fs::write(
            root.join(".kissconfig"),
            "[global]\n\
             duplication_enabled = false\n\
             \n\
[test]\n\
             test_coverage_threshold = 0\n\
             watch_settle_seconds = 0.005\n\
             orphan_detection = false\n\
             num_jobs = 1\n\
             \n\
             [test.max_unit_test_seconds]\n\
             \"*\" = 60\n\
             [python]\n\
             [rust]\n",
        )
        .unwrap();
        let mut warm = Command::new(env!("CARGO_BIN_EXE_kiss"));
        scrub_parent_coverage_env(&mut warm);
        preserve_toolchain_homes(&mut warm);
        warm.env("PYTHONDONTWRITEBYTECODE", "1");
        let status = warm
            .args(["test", "--lang", "python", "."])
            .current_dir(&root)
            .status()
            .expect("warm coverage-gap fixture");
        assert!(status.success(), "coverage-gap fixture warm failed");
        let add = kiss::scrubbed_git_command(&root)
            .args(["add", "-A"])
            .output()
            .expect("git add");
        assert!(add.status.success(), "git add failed");
        let commit = kiss::scrubbed_git_command(&root)
            .args(["commit", "-q", "-m", "init"])
            .output()
            .expect("git commit");
        assert!(commit.status.success(), "git commit failed");
        fs::create_dir_all(root.join(".kiss")).unwrap();
        fs::write(&stamp, root.to_string_lossy().as_bytes()).unwrap();
        root
    })
    .clone()
}

pub fn with_python_coverage_gap_repo<T>(f: impl FnOnce(&Path) -> T) -> T {
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = persistent_python_coverage_gap_repo();
    // Restore a clean threshold=0 config + sources before each use.
    fs::write(
        root.join("lib.py"),
        "def f():\n    return 0\ndef unused():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        root.join(".kissconfig"),
        "[global]\n\
         duplication_enabled = false\n\
         \n\
[test]\n\
         test_coverage_threshold = 0\n\
         watch_settle_seconds = 0.005\n\
         orphan_detection = false\n\
         num_jobs = 1\n\
         \n\
         [test.max_unit_test_seconds]\n\
         \"*\" = 60\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();
    let _ = fs::remove_dir_all(root.join(".kiss").join("watch"));
    // Drop warm seals so a threshold reload re-scores instead of echoing a sealed pass.
    if let Ok(entries) = fs::read_dir(root.join(".kiss").join("rslip_cache").join("hosts")) {
        for host in entries.flatten() {
            let _ = fs::remove_file(host.path().join("warm_hit_seal.json"));
        }
    }
    f(&root)
}

pub fn fresh_python_coverage_gap_repo() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().expect("coverage-gap tempdir");
    copy_repo_tree(&persistent_python_coverage_gap_repo(), tmp.path());
    let _ = fs::remove_dir_all(tmp.path().join(".kiss").join("watch"));
    if let Ok(entries) = fs::read_dir(tmp.path().join(".kiss").join("rslip_cache").join("hosts")) {
        for host in entries.flatten() {
            let _ = fs::remove_file(host.path().join("warm_hit_seal.json"));
        }
    }
    tmp
}

fn write_seeded_python_watch_sources(root: &Path) {
    fs::write(root.join("lib.py"), "def f():\n    return 0\n").unwrap();
    fs::write(
        root.join("test_lib.py"),
        "from lib import f\n\ndef test_f():\n    assert f() == 0\n",
    )
    .unwrap();
}

pub fn persistent_seeded_python_watch_repo() -> PathBuf {
    ensure_tmpfs();
    use std::sync::OnceLock;
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let root = std::env::temp_dir().join("kiss-seeded-python-watch-fixture-v2");
        if root.join(".git").join("HEAD").is_file() {
            write_seeded_python_watch_sources(&root);
            return root;
        }
        fs::create_dir_all(&root).expect("seeded python watch fixture root");
        let init = kiss::scrubbed_git_command(&root)
            .args(["init", "-q", "-b", "main"])
            .output()
            .expect("git init");
        assert!(init.status.success(), "git init failed");
        for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
            kiss::scrubbed_git_command(&root)
            .args(["config", kv.0, kv.1])
                .status()
                .expect("git config");
        }
        write_seeded_python_watch_sources(&root);
        fs::write(
            root.join(".kissconfig"),
            "[global]\n\
             duplication_enabled = false\n\
             \n\
[test]\n\
             test_coverage_threshold = 0\n\
             watch_settle_seconds = 0.005\n\
             orphan_detection = false\n\
             num_jobs = 1\n\
             \n\
             [test.max_unit_test_seconds]\n\
             \"*\" = 60\n\
             [python]\n\
             [rust]\n",
        )
        .unwrap();
        seed_python_runtime_coverage(
            &root,
            &[("test_lib.py::test_f", vec![("lib.py", vec![1, 2])])],
        );
        let add = kiss::scrubbed_git_command(&root)
            .args(["add", "-A"])
            .output()
            .expect("git add");
        assert!(add.status.success(), "git add failed");
        let commit = kiss::scrubbed_git_command(&root)
            .args(["commit", "-q", "-m", "init"])
            .output()
            .expect("git commit");
        assert!(commit.status.success(), "git commit failed");
        root
    })
    .clone()
}

pub fn fresh_seeded_python_watch_repo() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().expect("seeded python watch tempdir");
    copy_repo_tree(&persistent_seeded_python_watch_repo(), tmp.path());
    tmp
}

pub struct LockedSeededPythonWatchRepo {
    _tmp: tempfile::TempDir,
    path: PathBuf,
}

impl LockedSeededPythonWatchRepo {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Isolated clone of the seeded python watch fixture.
pub fn locked_seeded_python_watch_repo() -> LockedSeededPythonWatchRepo {
    let tmp = fresh_seeded_python_watch_repo();
    let path = tmp.path().to_path_buf();
    LockedSeededPythonWatchRepo { _tmp: tmp, path }
}

pub fn seed_python_runtime_coverage(repo: &Path, entries: &[PythonRuntimeCoverageSeed<'_>]) {
    ensure_tmpfs();
    seed_python_runtime_coverage_with_status(repo, entries, "Passed", 0);
}

pub fn seed_python_failed_runtime_coverage(
    repo: &Path,
    entries: &[PythonRuntimeCoverageSeed<'_>],
) {
    seed_python_runtime_coverage_with_status(repo, entries, "Failed", 1);
}

fn seed_python_runtime_coverage_with_status(
    repo: &Path,
    entries: &[PythonRuntimeCoverageSeed<'_>],
    status: &str,
    exit_code: i32,
) {
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
        write_seeded_rslip_entry(
            &repo,
            &cache_root,
            selector,
            coverage_files,
            &python_version,
            &pytest_version,
            &env,
            status,
            exit_code,
        );
    }
    selectors.sort();
    selectors.dedup();
    let manifest = serde_json::json!({
        "schema_version": "rslip-python-population-v1",
        "cache_schema_version": kiss::rslip::CACHE_SCHEMA_VERSION,
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

#[allow(clippy::too_many_arguments)]
fn write_seeded_rslip_entry(
    repo: &Path,
    cache_root: &Path,
    selector: &str,
    coverage_files: &[(&str, Vec<u32>)],
    python_version: &str,
    pytest_version: &str,
    env: &BTreeMap<String, String>,
    status: &str,
    exit_code: i32,
) {
    let req = kiss::rslip::RslipRequest {
        nodeid: selector.to_string(),
        cwd: repo.to_path_buf(),
        source_root: repo.to_path_buf(),
        python: PathBuf::from("python"),
        python_version: python_version.to_string(),
        pytest_version: pytest_version.to_string(),
        pytest_args: Vec::new(),
        env: env.clone(),
        cache_root: cache_root.to_path_buf(),
        force_rerun: false,
        timeout: None,
        content_fingerprint: None,
    };
    let fingerprint = kiss::rslip::cache_fingerprint_for_request(&req).unwrap();
    let files = coverage_files
        .iter()
        .map(|(file, lines)| {
            (
                coverage_seed_file(repo, file),
                lines.iter().copied().collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let coverage = kiss::rslip::LineCoverage {
        files: files.clone(),
    };
    let covered_digests =
        kiss::rslip::covered_file_digests_for(repo, selector, &coverage).unwrap_or_default();
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

pub fn seed_rust_runtime_coverage(repo: &Path, entries: &[RustRuntimeCoverageSeed<'_>]) {
    let repo = repo.canonicalize().unwrap();
    let selectors = sorted_unique_selectors(entries.iter().map(|(selector, _)| *selector));
    let mut req = rust_runtime_coverage_request(&repo, &selectors);
    kiss::rust_llvm_cov_runner::resolve_batch_request_runners(&mut req).unwrap();
    let tools = rust_runtime_coverage_tool_identity(&repo);
    let identity = kiss::rust_llvm_cov_runner::batch_identity(&req, &tools).unwrap();
    for (selector, coverage_files) in entries {
        let fingerprint = kiss::rust_llvm_cov_runner::entry_fingerprint(
            &identity.input_digest,
            &req,
            &tools,
            selector,
        );
        let files = coverage_files
            .iter()
            .map(|(file, lines)| {
                (
                    coverage_seed_file(repo.as_path(), file),
                    lines.iter().copied().collect::<BTreeSet<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let outcome = kiss::rust_llvm_cov_runner::RustLlvmCovOutcome {
            selector: (*selector).to_string(),
            status: kiss::rpytest_runner::TestStatus::Passed,
            exit_code: Some(0),
            duration: std::time::Duration::from_millis(1),
            coverage: kiss::rust_llvm_cov_runner::RustLineCoverage { files },
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: kiss::rust_llvm_cov_runner::RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        };
        let entry = kiss::rust_llvm_cov_runner::RustCovCacheEntry::from_outcome(
            &outcome,
            &identity.generation_fingerprint,
        );
        kiss::rust_llvm_cov_runner::store_rust_cov_cache_entry(
            &req.cache_root,
            &fingerprint,
            &entry,
        )
        .unwrap();
    }
    kiss::rust_llvm_cov_runner::publish_derived_state(&req, &tools, &identity, &selectors, false)
        .unwrap();
}

fn rust_runtime_coverage_request(
    repo: &Path,
    selectors: &[String],
) -> kiss::rust_llvm_cov_runner::RustCoverageBatchRequest {
    // Match live `rust_coverage_batch_request_from_parts`: empty runners, then resolve.
    kiss::rust_llvm_cov_runner::RustCoverageBatchRequest {
        cwd: repo.to_path_buf(),
        source_root: repo.to_path_buf(),
        cargo: PathBuf::from("cargo"),
        cache_root: repo.join(".kiss").join("rust_llvm_cov_cache"),
        logical_selectors: selectors.to_vec(),
        cargo_args: vec!["--workspace".to_string()],
        test_args: Vec::new(),
        env: relevant_rust_env(),
        force_rerun: false,
        force_rerun_selectors: Vec::new(),
        jobs: 1,
        generated_config: repo
            .join(".kiss")
            .join("rust_llvm_cov_cache")
            .join("runs")
            .join("test-seed")
            .join("nextest.toml"),
        population_publication_selectors: Some(selectors.to_vec()),
        delegated_runners: std::collections::BTreeMap::new(),
        runner_map_fingerprint: String::new(),
        host_platform: String::new(),
        coverage_output_mode: kiss::rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
        selector_timeout_millis: std::collections::BTreeMap::new(),
        cache_policy: kiss::test_cache_policy::TestCachePolicy::default(),
    }
}

fn rust_runtime_coverage_tool_identity(
    repo: &Path,
) -> kiss::rust_llvm_cov_runner::RustCoverageToolIdentity {
    // Match live `version_with_meta_tag` so seeded generation fingerprints hit.
    kiss::rust_llvm_cov_runner::RustCoverageToolIdentity {
        cargo_version: version_with_meta_tag(
            command_output(repo, "cargo", &["--version"]),
            Path::new("cargo"),
        ),
        llvm_cov_version: version_with_meta_tag(
            command_output(repo, "cargo", &["llvm-cov", "--version"]),
            Path::new("cargo-llvm-cov"),
        ),
        rustc_version: version_with_meta_tag(
            command_output(repo, "rustc", &["-Vv"]),
            Path::new("rustc"),
        ),
        cargo_nextest_version: version_with_meta_tag(
            command_output(repo, "cargo", &["nextest", "--version"]),
            Path::new("cargo-nextest"),
        ),
    }
}

fn version_with_meta_tag(version: String, program: &Path) -> String {
    let resolved = resolve_on_path(program).unwrap_or_else(|| program.to_path_buf());
    let Ok(meta) = fs::metadata(&resolved) else {
        return version;
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        meta.ino()
    };
    #[cfg(not(unix))]
    let inode = 0u64;
    format!("{version}#{:x}-{:x}-{:x}", meta.len(), mtime, inode)
}

fn resolve_on_path(program: &Path) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(program);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    })
}

fn relevant_rust_env() -> BTreeMap<String, String> {
    // Mirror live `relevant_rust_batch_env`, but omit coverage-instrumentation keys that
    // `scrub_parent_coverage_env` strips from nested kiss. Empty env was wrong: generation
    // fingerprints hash `resolved_identity_tools(req.env)` which needs PATH, so seeds
    // never matched and `--coverage-all` cold-ran llvm-cov (~60s+ TIMEOUT under suite load).
    const CHILD_KEYS: &[&str] = &[
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "SHELL",
        "TMPDIR",
        "TMP",
        "TEMP",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "LD_LIBRARY_PATH",
        "CC",
        "CXX",
        "CONDA_PREFIX",
        "PKG_CONFIG_PATH",
    ];
    const SURVIVING_COVERAGE_KEYS: &[&str] =
        &["KISS_RUST_LLVM_COV_HOLD_BEFORE_GO_MS", "CMAKE_PREFIX_PATH"];
    let mut env = kiss::env_map_from_allowlist(CHILD_KEYS);
    env.extend(kiss::env_map_from_allowlist(SURVIVING_COVERAGE_KEYS));
    env.extend(kiss::cargo_target_linker_env());
    if !env.contains_key("CMAKE_PREFIX_PATH")
        && let Some(conda) = env.get("CONDA_PREFIX").cloned()
    {
        env.insert("CMAKE_PREFIX_PATH".to_string(), conda);
    }
    env
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
