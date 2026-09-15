use std::collections::BTreeMap;
use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use fs2::FileExt;
use kiss::rpytest_runner::TestStatus;
use kiss::rust_llvm_cov_runner::RustLineCoverage;
use tempfile::TempDir;

use crate::test_runner::PlannedSelectors;
use crate::test_runner::coverage_decision::SelectionBasis;
use crate::test_runner::rust_coverage_index::{
    rebuild_rust_coverage_index, write_rust_population_manifest_for_args, write_test_entry,
};

use super::git::{commit_all, ensure_main_branch, git_in, git_stdout, init_git, init_git_dir};

pub(crate) const RS_COVERING_SELECTOR: &str = "tests::gets_value";

/// True when seeded population selection-context matches the current batch identity.
/// Persistent fixtures must not reuse a prior publish after allowlisted env drift
/// (e.g. kiss llvm-cov `RUSTFLAGS=-C instrument-coverage`).
pub(crate) fn seeded_population_matches_current_context(root: &Path) -> bool {
    let population = root
        .join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("population.json");
    let Ok(bytes) = fs::read(&population) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    let Some(stored) = value
        .get("selection_context_fingerprint")
        .and_then(|v| v.as_str())
    else {
        return false;
    };
    let Ok(identity) =
        crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(root, &[])
    else {
        return false;
    };
    stored == identity.selection_context_fingerprint
}

struct FixtureLock {
    _mutex: MutexGuard<'static, ()>,
    _file: std::fs::File,
}

fn warm_demo_mutex() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn warm_committed_mutex() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn base_historical_mutex() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_fixture(mutex: &'static Mutex<()>, lock_name: &str) -> FixtureLock {
    let _mutex = mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let lock_path = std::env::temp_dir().join(lock_name);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap_or_else(|e| panic!("open {}: {e}", lock_path.display()));
    file.lock_exclusive()
        .unwrap_or_else(|e| panic!("lock {}: {e}", lock_path.display()));
    FixtureLock {
        _mutex,
        _file: file,
    }
}

fn population_json(root: &Path) -> PathBuf {
    root.join(".kiss")
        .join("rust_llvm_cov_cache")
        .join("population.json")
}

fn warm_demo_path() -> PathBuf {
    std::env::temp_dir().join("kiss-warm-demo-fixture")
}

fn warm_committed_path() -> PathBuf {
    std::env::temp_dir().join("kiss-warm-committed-fixture")
}

fn base_historical_path() -> PathBuf {
    std::env::temp_dir().join("kiss-base-historical-fixture")
}

fn persistent_cargo_target(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("kiss-test-targets").join(name);
    std::fs::create_dir_all(&dir).expect("persistent cargo target");
    dir
}

fn write_shared_cargo_target_config(root: &Path, target_name: &str) {
    let config_dir = root.join(".cargo");
    std::fs::create_dir_all(&config_dir).expect("cargo config dir");
    let target_dir = persistent_cargo_target(target_name);
    std::fs::write(
        config_dir.join("config.toml"),
        format!("[build]\ntarget-dir = \"{}\"\n", target_dir.display()),
    )
    .expect("cargo config");
}

fn prebuild_cargo_tests(root: &Path, target_name: &str) {
    let target_dir = persistent_cargo_target(target_name);
    let status = std::process::Command::new("cargo")
        .args(["test", "--no-run", "-q"])
        .current_dir(root)
        .env("CARGO_TARGET_DIR", &target_dir)
        .status()
        .expect("cargo prebuild");
    assert!(status.success(), "cargo prebuild failed for {}", root.display());
}

pub(crate) fn lib_source(value: u32) -> String {
    format!(
        "pub fn value() -> u32 {{ {value} }}\n#[cfg(test)]\nmod tests {{ #[test] fn gets_value() {{ assert_eq!(super::value(), {value}); }} }}\n"
    )
}

pub(crate) fn publish_lib_population(root: &Path) {
    write_test_entry(
        root,
        "abc",
        RS_COVERING_SELECTOR,
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(
                "src/lib.rs".to_string(),
                std::collections::BTreeSet::from([1]),
            )]),
        },
    );
    // rebuild publishes the population from entry selectors; no second publish.
    rebuild_rust_coverage_index(root).unwrap();
}

/// After `clone_warm_committed_repo`, restamp cached entries for the clone's identity
/// without rewriting coverage payloads or rebuilding the line index from scratch.
#[allow(dead_code)]
pub(crate) fn republish_cloned_lib_population(root: &Path) {
    let test_args: &[String] = &[];
    let identity = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
        root, test_args,
    )
    .expect("batch identity for cloned republish");
    let cache_root = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(root);
    let entries_dir = cache_root.join("entries");
    if entries_dir.is_dir() {
        for entry in fs::read_dir(&entries_dir)
            .expect("entries dir")
            .flatten()
        {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            if value.get("generation_fingerprint").is_some() {
                value["generation_fingerprint"] =
                    serde_json::Value::String(identity.generation_fingerprint.clone());
                fs::write(&path, serde_json::to_vec(&value).expect("entry json")).expect("write entry");
            }
        }
        kiss::rust_llvm_cov_runner::invalidate_entry_state(&cache_root);
    }
    write_rust_population_manifest_for_args(root, &[RS_COVERING_SELECTOR.to_string()], test_args)
        .expect("republish cloned lib population");
}

fn write_demo_crate(root: &Path, value: u32) -> PathBuf {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();

    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let lib = root.join("src").join("lib.rs");
    fs::write(&lib, lib_source(value)).unwrap();
    lib
}

pub(crate) fn warm_committed_rust_demo(tmp: &TempDir) -> PathBuf {
    init_git(tmp);
    ensure_main_branch(tmp.path());
    write_shared_cargo_target_config(tmp.path(), "warm-committed-demo");
    let lib = write_demo_crate(tmp.path(), 1);
    publish_lib_population(tmp.path());
    commit_all(tmp.path(), "warm");
    lib
}

fn ensure_warm_committed_repo() -> PathBuf {
    let root = warm_committed_path();
    let stamp = root.join(".kiss").join("fixture_inplace_ok");
    let usable = population_json(&root).is_file()
        && stamp.is_file()
        && fs::read_to_string(&stamp).ok().as_deref() == Some(root.to_string_lossy().as_ref())
        && seeded_population_matches_current_context(&root);
    if usable {
        return root;
    }
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("warm committed fixture root");
    init_git_dir(&root);
    ensure_main_branch(&root);
    write_shared_cargo_target_config(&root, "warm-committed-demo");
    let _lib = write_demo_crate(&root, 1);
    publish_lib_population(&root);
    commit_all(&root, "warm");
    prebuild_cargo_tests(&root, "warm-committed-demo");
    fs::create_dir_all(root.join(".kiss")).expect("kiss dir");
    fs::write(&stamp, root.to_string_lossy().as_bytes()).expect("fixture stamp");
    root
}

pub(crate) fn clone_warm_committed_repo(dst: &Path) -> PathBuf {
    let _lock = lock_fixture(warm_committed_mutex(), "kiss-warm-committed-fixture.lock");
    copy_repo_tree(&ensure_warm_committed_repo(), dst);
    dst.join("src").join("lib.rs")
}

struct RestoreLibSource {
    path: PathBuf,
    contents: String,
}

impl Drop for RestoreLibSource {
    fn drop(&mut self) {
        let _ = fs::write(&self.path, &self.contents);
    }
}

/// Use the persistent warm committed fixture in-process (no clone/republish).
pub(crate) fn with_locked_warm_committed_repo<T>(
    f: impl FnOnce(&Path, PathBuf) -> T,
) -> T {
    let _lock = lock_fixture(warm_committed_mutex(), "kiss-warm-committed-fixture.lock");
    let repo = ensure_warm_committed_repo();
    let lib = repo.join("src").join("lib.rs");
    // Always restore to the committed baseline before capturing restore state.
    edit_rust_covered_source(&lib, 1);
    let contents = fs::read_to_string(&lib).expect("warm committed lib.rs");
    let _restore = RestoreLibSource {
        path: lib.clone(),
        contents,
    };
    f(&repo, lib)
}

fn ensure_base_historical_repo() -> PathBuf {
    let root = base_historical_path();
    if root.join(".git").join("HEAD").is_file()
        && population_json(&root).is_file()
        && seeded_population_matches_current_context(&root)
    {
        return root;
    }
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("base historical fixture root");
    init_git_dir(&root);
    ensure_main_branch(&root);
    write_shared_cargo_target_config(&root, "base-historical-demo");
    let lib = write_demo_crate(&root, 1);
    commit_all(&root, "baseline");
    fs::write(
        root.join("src").join("historical.rs"),
        "pub fn historical() -> u32 { 1 }\n",
    )
    .unwrap();
    assert!(
        git_in(&root)
            .args(["add", "src/historical.rs"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(&root)
            .args(["commit", "-m", "historical"])
            .status()
            .unwrap()
            .success()
    );
    publish_lib_population(&root);
    edit_rust_covered_source(&lib, 2);
    root
}

fn base_historical_repo_lock() -> FixtureLock {
    lock_fixture(base_historical_mutex(), "kiss-base-historical-fixture.lock")
}

/// Use the persistent base/historical fixture in-place (no clone).
pub(crate) fn with_locked_base_historical_repo<T>(
    f: impl FnOnce(&Path, String, PathBuf) -> T,
) -> T {
    let _lock = base_historical_repo_lock();
    let repo = ensure_base_historical_repo();
    let lib = repo.join("src").join("lib.rs");
    let baseline = git_stdout(&repo, &["rev-parse", "HEAD~1"]);
    // In-place fixture already has value=1 coverage + dirty value=2 tree.
    edit_rust_covered_source(&lib, 2);
    f(&repo, baseline, lib)
}

#[allow(dead_code)]
pub(crate) fn clone_base_historical_repo(dst: &Path) -> (String, PathBuf) {
    let _lock = base_historical_repo_lock();
    copy_repo_tree(&ensure_base_historical_repo(), dst);
    let baseline = git_stdout(dst, &["rev-parse", "HEAD~1"]);
    let lib = dst.join("src").join("lib.rs");
    (baseline, lib)
}

pub(crate) fn clone_warm_demo_repo(dst: &Path) -> PathBuf {
    let _lock = warm_demo_repo_lock();
    copy_repo_tree(&ensure_warm_demo_repo(), dst);
    dst.join("src").join("lib.rs")
}

fn warm_demo_repo_lock() -> FixtureLock {
    lock_fixture(warm_demo_mutex(), "kiss-warm-demo-fixture.lock")
}

fn ensure_warm_demo_repo() -> PathBuf {
    let root = warm_demo_path();
    if root.join("Cargo.toml").is_file()
        && population_json(&root).is_file()
        && seeded_population_matches_current_context(&root)
    {
        return root;
    }
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("warm demo fixture root");
    write_warm_demo_repo(&root);
    root
}

/// Use the persistent warm demo fixture in-place (no clone/republish).
pub(crate) fn with_locked_warm_demo_repo<T>(f: impl FnOnce(&Path, PathBuf) -> T) -> T {
    let _lock = warm_demo_repo_lock();
    let repo = ensure_warm_demo_repo();
    let lib = repo.join("src").join("lib.rs");
    let contents = fs::read_to_string(&lib).expect("warm demo lib.rs");
    let _restore = RestoreLibSource {
        path: lib.clone(),
        contents,
    };
    edit_rust_covered_source(&lib, 2);
    f(&repo, lib)
}

fn write_warm_demo_repo(root: &Path) -> PathBuf {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let lib = root.join("src").join("lib.rs");
    fs::write(
        &lib,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() { assert_eq!(super::value(), 1); } }\n",
    )
    .unwrap();
    publish_lib_population(root);
    lib
}

pub(crate) fn clone_row_b_committed_repo(dst: &Path) {
    copy_repo_tree(&persistent_row_b_committed_repo(), dst);
}

fn copy_repo_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("copy destination");
    let status = std::process::Command::new("cp")
        .args(["-a", &format!("{}/.", src.display()), &format!("{}/", dst.display())])
        .status()
        .expect("cp");
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
        let Ok(entries) = std::fs::read_dir(&dir) else {
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
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if !text.contains(old.as_ref()) {
                continue;
            }
            std::fs::write(&path, text.replace(old.as_ref(), new.as_ref())).expect("retarget cache");
        }
    }
}

pub(crate) fn persistent_row_b_committed_repo() -> PathBuf {
    use std::sync::OnceLock;
    static REPO: OnceLock<PathBuf> = OnceLock::new();
    REPO.get_or_init(|| {
        let root = std::env::temp_dir().join("kiss-row-b-committed-fixture");
        if root.join(".git").join("HEAD").is_file() {
            return root;
        }
        let tmp = TempDir::new().expect("row-b fixture tempdir");
        let lib = warm_committed_rust_demo(&tmp);
        let test_file = tmp.path().join("src").join("extra_test.rs");
        std::fs::write(&test_file, "#[test]\nfn only_extra() {}\n").unwrap();
        let mut lib_src = std::fs::read_to_string(&lib).unwrap();
        lib_src.push_str("#[cfg(test)]\nmod extra_test;\n");
        std::fs::write(&lib, lib_src).unwrap();
        assert!(
            git_in(tmp.path())
                .args(["add", "src/extra_test.rs", "src/lib.rs"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            git_in(tmp.path())
                .args(["commit", "-q", "-m", "add-extra-test"])
                .status()
                .unwrap()
                .success()
        );
        std::fs::create_dir_all(&root).expect("row-b fixture root");
        copy_repo_tree(tmp.path(), &root);
        root
    })
    .clone()
}

pub(crate) fn edit_rust_covered_source(lib: &Path, value: u32) {
    fs::write(lib, lib_source(value)).unwrap();
}

#[allow(dead_code)]
pub(crate) fn warm_base_demo_with_historical_source(tmp: &TempDir) -> (String, PathBuf) {
    init_git(tmp);
    ensure_main_branch(tmp.path());
    write_shared_cargo_target_config(tmp.path(), "base-historical-demo");
    let lib = write_demo_crate(tmp.path(), 1);
    publish_lib_population(tmp.path());
    commit_all(tmp.path(), "baseline");
    let baseline = git_stdout(tmp.path(), &["rev-parse", "HEAD"]);
    fs::write(
        tmp.path().join("src").join("historical.rs"),
        "pub fn historical() -> u32 { 1 }\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "src/historical.rs"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "historical"])
            .status()
            .unwrap()
            .success()
    );
    publish_lib_population(tmp.path());
    edit_rust_covered_source(&lib, 2);
    (baseline, lib)
}

pub(crate) fn assert_base_delta_plan(planned: &PlannedSelectors, lib: &Path) {
    assert_eq!(planned.selection_basis.rust, SelectionBasis::ReusablePrior);
    assert!(!planned.population_required.rust);
    assert_eq!(planned.source_paths.rust, vec![lib.to_path_buf()]);
    assert!(
        planned.vcs_source_paths.rust >= 2,
        "base/main: VCS range must include historical + edited paths, got {}",
        planned.vcs_source_paths.rust
    );
    assert_eq!(planned.snapshot_delta_modified.rust, 1);
    assert!(!planned.snapshot_delta_structural.rust);
    assert_eq!(planned.sel.rust, vec![RS_COVERING_SELECTOR.to_string()]);
}
