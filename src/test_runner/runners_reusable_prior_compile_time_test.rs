use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use kiss::rpytest_runner::TestStatus;
use kiss::rust_llvm_cov_runner::RustLineCoverage;

use crate::test_runner::coverage_decision::SelectionBasis;
use crate::test_runner::runners::combined_selectors;
use crate::test_runner::rust_coverage_index::{
    rebuild_rust_coverage_index, write_rust_population_manifest_for_args, write_test_entry,
};

fn shared_cargo_target(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("kiss-test-targets").join(name);
    fs::create_dir_all(&dir).expect("shared cargo target");
    dir
}

fn write_shared_cargo_target_config(root: &Path, name: &str) {
    let config_dir = root.join(".cargo");
    fs::create_dir_all(&config_dir).expect("cargo config dir");
    let target_dir = shared_cargo_target(name);
    fs::write(
        config_dir.join("config.toml"),
        format!("[build]\ntarget-dir = \"{}\"\n", target_dir.display()),
    )
    .expect("cargo config");
}

fn prebuild_cargo_tests(root: &Path, name: &str) {
    let target_dir = shared_cargo_target(name);
    let status = std::process::Command::new("cargo")
        .args(["test", "--no-run", "-q"])
        .current_dir(root)
        .env("CARGO_TARGET_DIR", &target_dir)
        .status()
        .expect("cargo prebuild");
    assert!(
        status.success(),
        "cargo prebuild failed for {}",
        root.display()
    );
}

fn compile_time_fixture_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn lock_compile_time_fixture(lock_name: &str) -> std::fs::File {
    let lock_path = std::env::temp_dir().join(lock_name);
    let file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap_or_else(|e| panic!("open {}: {e}", lock_path.display()));
    use fs2::FileExt;
    file.lock_exclusive()
        .unwrap_or_else(|e| panic!("lock {}: {e}", lock_path.display()));
    file
}

struct RestoredPaths {
    snapshots: Vec<(PathBuf, Vec<u8>)>,
}

impl RestoredPaths {
    fn snapshot(&mut self, path: &Path) {
        self.snapshots
            .push((path.to_path_buf(), fs::read(path).expect("snapshot read")));
    }
}

impl Drop for RestoredPaths {
    fn drop(&mut self) {
        for (path, bytes) in &self.snapshots {
            fs::write(path, bytes).expect("restore snapshot");
        }
    }
}

fn ensure_app_builder_workspace() -> PathBuf {
    let root = std::env::temp_dir().join("kiss-app-builder-fixture");
    if root.join("Cargo.toml").is_file() {
        if !crate::test_runner::test_mode_fixtures::seeded_population_matches_current_context(&root)
        {
            warm_app_builder_workspace(&root);
        }
        return root;
    }
    fs::create_dir_all(&root).expect("builder fixture root");
    write_shared_cargo_target_config(&root, "app-builder-demo");
    warm_app_builder_workspace(&root);
    prebuild_cargo_tests(&root, "app-builder-demo");
    root
}

fn ensure_app_proc_macro_workspace() -> PathBuf {
    let root = std::env::temp_dir().join("kiss-app-proc-macro-fixture");
    if root.join("Cargo.toml").is_file() {
        if !crate::test_runner::test_mode_fixtures::seeded_population_matches_current_context(&root)
        {
            warm_app_proc_macro_workspace(&root);
        }
        return root;
    }
    fs::create_dir_all(&root).expect("proc-macro fixture root");
    write_shared_cargo_target_config(&root, "app-proc-macro-demo");
    warm_app_proc_macro_workspace(&root);
    prebuild_cargo_tests(&root, "app-proc-macro-demo");
    root
}

#[test]
fn build_script_edit_forces_population_while_ordinary_lib_stays_reusable() {
    let _mutex = compile_time_fixture_lock();
    let _file = lock_compile_time_fixture("kiss-app-builder-fixture.lock");
    let root = ensure_app_builder_workspace();
    let lib = root.join("app").join("src").join("lib.rs");
    let build_rs = root.join("builder").join("build.rs");
    let mut restore = RestoredPaths {
        snapshots: Vec::new(),
    };
    restore.snapshot(&lib);
    restore.snapshot(&build_rs);
    fs::write(
        &lib,
        "pub fn value() -> u32 { 2 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() { assert_eq!(super::value(), 2); } }\n",
    )
    .unwrap();
    let ordinary = combined_selectors(
        &root,
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();
    assert!(!ordinary.population_required.rust);
    assert_eq!(ordinary.selection_basis.rust, SelectionBasis::ReusablePrior);

    fs::write(
        &build_rs,
        "fn main() { println!(\"cargo:rerun-if-env-changed=BUILD_SCRIPT_INPUT\"); }\n",
    )
    .unwrap();
    let compile_time = combined_selectors(
        &root,
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();
    assert!(compile_time.population_required.rust);
    assert_eq!(
        compile_time.selection_basis.rust,
        SelectionBasis::Population
    );
}

#[test]
fn manifest_only_compile_time_edit_forces_population() {
    let _mutex = compile_time_fixture_lock();
    let _file = lock_compile_time_fixture("kiss-app-builder-fixture.lock");
    let root = ensure_app_builder_workspace();
    let manifest = root.join("builder").join("Cargo.toml");
    let mut restore = RestoredPaths {
        snapshots: Vec::new(),
    };
    restore.snapshot(&manifest);
    fs::write(
        &manifest,
        "[package]\nname='builder'\nversion='0.1.1'\nedition='2024'\nbuild='build.rs'\n",
    )
    .unwrap();

    let compile_time = combined_selectors(
        &root,
        std::slice::from_ref(&manifest),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();

    assert!(compile_time.population_required.rust);
    assert_eq!(
        compile_time.selection_basis.rust,
        SelectionBasis::Population
    );
}

#[test]
fn proc_macro_edit_forces_population_while_ordinary_lib_stays_reusable() {
    let _mutex = compile_time_fixture_lock();
    let _file = lock_compile_time_fixture("kiss-app-proc-macro-fixture.lock");
    let root = ensure_app_proc_macro_workspace();
    let lib = root.join("app").join("src").join("lib.rs");
    let macros = root.join("macros").join("src").join("lib.rs");
    let mut restore = RestoredPaths {
        snapshots: Vec::new(),
    };
    restore.snapshot(&lib);
    restore.snapshot(&macros);
    fs::write(
        &lib,
        "pub fn value() -> u32 { 2 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() { assert_eq!(super::value(), 2); } }\n",
    )
    .unwrap();
    let ordinary = combined_selectors(
        &root,
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();
    assert!(!ordinary.population_required.rust);
    assert_eq!(ordinary.selection_basis.rust, SelectionBasis::ReusablePrior);

    fs::write(
        &macros,
        "extern crate proc_macro;\nuse proc_macro::TokenStream;\n#[proc_macro]\npub fn mark(input: TokenStream) -> TokenStream { input }\n",
    )
    .unwrap();
    let compile_time = combined_selectors(
        &root,
        std::slice::from_ref(&lib),
        &[],
        &BTreeMap::new(),
        &[],
        None,
        &[],
    )
    .unwrap();
    assert!(compile_time.population_required.rust);
    assert_eq!(
        compile_time.selection_basis.rust,
        SelectionBasis::Population
    );
}

fn write_workspace_cargo_lock(root: &Path, members: &[(&str, &str)]) {
    let mut lock = String::from("version = 4\n\n");
    for (name, version) in members {
        lock.push_str(&format!(
            "[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n\n"
        ));
    }
    fs::write(root.join("Cargo.lock"), lock).unwrap();
}

fn warm_app_builder_workspace(root: &Path) -> std::path::PathBuf {
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"app\", \"builder\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    write_workspace_cargo_lock(root, &[("app", "0.1.0"), ("builder", "0.1.0")]);
    fs::create_dir_all(root.join("app").join("src")).unwrap();
    fs::create_dir_all(root.join("builder").join("src")).unwrap();
    fs::write(
        root.join("app").join("Cargo.toml"),
        "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(
        root.join("builder").join("Cargo.toml"),
        "[package]\nname='builder'\nversion='0.1.0'\nedition='2024'\nbuild='build.rs'\n",
    )
    .unwrap();
    fs::write(
        root.join("builder").join("build.rs"),
        "fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }\n",
    )
    .unwrap();
    fs::write(
        root.join("builder").join("src").join("lib.rs"),
        "pub fn marker() -> u32 { 1 }\n",
    )
    .unwrap();
    warm_app_lib_population(root)
}

fn warm_app_proc_macro_workspace(root: &Path) -> std::path::PathBuf {
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"app\", \"macros\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    write_workspace_cargo_lock(root, &[("app", "0.1.0"), ("macros", "0.1.0")]);
    fs::create_dir_all(root.join("app").join("src")).unwrap();
    fs::create_dir_all(root.join("macros").join("src")).unwrap();
    fs::write(
        root.join("app").join("Cargo.toml"),
        "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(
        root.join("macros").join("Cargo.toml"),
        "[package]\nname='macros'\nversion='0.1.0'\nedition='2024'\n[lib]\nproc-macro = true\n",
    )
    .unwrap();
    fs::write(
        root.join("macros").join("src").join("lib.rs"),
        "extern crate proc_macro;\nuse proc_macro::TokenStream;\n#[proc_macro]\npub fn identity(input: TokenStream) -> TokenStream { input }\n",
    )
    .unwrap();
    warm_app_lib_population(root)
}

fn warm_app_lib_population(root: &Path) -> std::path::PathBuf {
    let lib = root.join("app").join("src").join("lib.rs");
    fs::write(
        &lib,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() { assert_eq!(super::value(), 1); } }\n",
    )
    .unwrap();
    write_test_entry(
        root,
        "abc",
        "tests::gets_value",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(
                "app/src/lib.rs".to_string(),
                std::collections::BTreeSet::from([1]),
            )]),
        },
    );
    rebuild_rust_coverage_index(root).unwrap();
    write_rust_population_manifest_for_args(root, &["tests::gets_value".to_string()], &[]).unwrap();
    lib
}
