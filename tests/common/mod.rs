#![allow(dead_code)]

use fs2::FileExt;
use kiss::parsing::{ParsedFile, create_parser, parse_file};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use tree_sitter::Node;

mod python_seed_helpers;
use python_seed_helpers::{
    python_entries_fingerprint, python_rslip_cache_root_for_repo,
    python_seeded_population_is_current, python_source_input_fingerprint,
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
pub fn assert_seeded_python_runtime_coverage_stale_after(repo: &Path, apply_change: impl FnOnce()) {
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

/// Strip parent build and profiling env so nested cargo work is not inflated under suite load.
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
            "[global]\nduplication_enabled = false\n[test]\norphan_detection = false\n[python]\n[rust]\n",
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
        // Unique per process so parallel nextest workers cannot share/mutate one fixture.
        let root = std::env::temp_dir().join(format!(
            "kiss-python-failure-fixture-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(".kissconfig"),
            "[global]\nduplication_enabled = false\n[test]\norphan_detection = false\n[python]\n[rust]\n",
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
        .args([
            "-a",
            &format!("{}/.", src.display()),
            &format!("{}/", dst.display()),
        ])
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
         orphan_detection = false\n\
         num_jobs = 1\n\
         \n\
         [test.max_unit_test_seconds]\n\
         \"*\" = 60\n\
         [python]\n\
         [rust]\n",
    )
    .unwrap();
    f(&root)
}

pub fn fresh_python_coverage_gap_repo() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().expect("coverage-gap tempdir");
    copy_repo_tree(&persistent_python_coverage_gap_repo(), tmp.path());
    tmp
}

fn write_seeded_python_sources(root: &Path) {
    fs::write(root.join("lib.py"), "def f():\n    return 0\n").unwrap();
    fs::write(
        root.join("test_lib.py"),
        "from lib import f\n\ndef test_f():\n    assert f() == 0\n",
    )
    .unwrap();
}

/// Cross-process lock for the shared seeded-python fixture.
/// nextest workers are separate OS processes; an in-process Mutex does not
/// serialize rewrite of the persistent tree against concurrent `cp -a` clones.
/// Uses an exclusive flock (same class as production kiss-plan stores / force-python
/// fixture). A mkdir lock with timed steal can interrupt a live holder still
/// cloning under nextest -j12 load.
struct SeededPythonFixtureLock {
    _file: fs::File,
}

fn seeded_python_fixture_lock() -> SeededPythonFixtureLock {
    ensure_tmpfs();
    let path = std::env::temp_dir().join("kiss-seeded-python-fixture-v4.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .unwrap_or_else(|err| panic!("seeded python fixture lock {}: {err}", path.display()));
    file.lock_exclusive().unwrap_or_else(|err| {
        panic!(
            "seeded python fixture lock_exclusive {}: {err}",
            path.display()
        )
    });
    SeededPythonFixtureLock { _file: file }
}

/// Caller must hold `seeded_python_fixture_lock`.
fn persistent_seeded_python_repo_unlocked() -> PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        // Ship a .gitignore so `.kiss/` / `target/` are never tracked; tracked
        // state dirt would flap commit stamps.
        let root = std::env::temp_dir().join("kiss-seeded-python-fixture-v4");
        if root.join(".git").join("HEAD").is_file() {
            // Reset sources under the flock so concurrent clones cannot tear a
            // mid-write lib.py / test_lib.py pair.
            write_seeded_python_sources(&root);
            return root;
        }
        fs::create_dir_all(&root).expect("seeded python fixture root");
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
            root.join(".gitignore"),
            "target/\n.kiss/\n__pycache__/\n*.pyc\n",
        )
        .unwrap();
        write_seeded_python_sources(&root);
        fs::write(
            root.join(".kissconfig"),
            "[global]\n\
             duplication_enabled = false\n\
             \n\
[test]\n\
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

pub fn persistent_seeded_python_repo() -> PathBuf {
    let _lock = seeded_python_fixture_lock();
    persistent_seeded_python_repo_unlocked()
}

pub fn fresh_seeded_python_repo() -> tempfile::TempDir {
    // Hold the lock across clone so peers cannot rewrite sources mid-`cp -a`.
    let _lock = seeded_python_fixture_lock();
    let tmp = tempfile::TempDir::new().expect("seeded python tempdir");
    copy_repo_tree(&persistent_seeded_python_repo_unlocked(), tmp.path());
    tmp
}

pub struct LockedSeededPythonRepo {
    _tmp: tempfile::TempDir,
    path: PathBuf,
}

impl LockedSeededPythonRepo {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Isolated clone of the seeded python fixture.
pub fn locked_seeded_python_repo() -> LockedSeededPythonRepo {
    let tmp = fresh_seeded_python_repo();
    let path = tmp.path().to_path_buf();
    LockedSeededPythonRepo { _tmp: tmp, path }
}

pub fn seed_python_runtime_coverage(repo: &Path, entries: &[PythonRuntimeCoverageSeed<'_>]) {
    ensure_tmpfs();
    seed_python_runtime_coverage_with_status(repo, entries, "Passed", 0);
}

pub fn seed_python_failed_runtime_coverage(repo: &Path, entries: &[PythonRuntimeCoverageSeed<'_>]) {
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
    fs::create_dir_all(&cache_root).unwrap();
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
        "entries_fingerprint": python_entries_fingerprint(&repo),
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
    };
    let identity = kiss::rslip::record_identity_for_request(&req).unwrap();
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
    let deps = kiss::rslip::covered_file_digests_for(repo, selector, &coverage).unwrap_or_default();
    let record = kiss::test_records::TestRecord {
        schema: kiss::test_records::RECORD_SCHEMA.to_string(),
        language: "python".to_string(),
        test_id: selector.to_string(),
        identity,
        deps,
        status: serde_json::from_value(serde_json::json!(status)).unwrap(),
        exit_code: Some(exit_code),
        duration: std::time::Duration::from_millis(1),
        covered: files,
    };
    kiss::test_records::store_record(&kiss::rslip::python_records_dir(repo), &record).unwrap();
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
