//! Structure regression tests for Proposal 2 call-flow packages.

use std::path::PathBuf;

fn crate_src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn verb_directories_and_substrate_files_exist() {
    let src = crate_src_dir();
    for dir in ["plan", "execute_or_reuse", "publish_derived"] {
        assert!(
            src.join(dir).is_dir(),
            "missing verb directory src/{dir}"
        );
    }
    for file in ["rust_cov_cache.rs", "file_lock.rs", "kiss_profraw.rs"] {
        assert!(
            src.join(file).is_file(),
            "missing substrate file src/{file}"
        );
    }
}

#[test]
fn stable_public_api_names_resolve_at_crate_root() {

    use crate::{
        CoverageOutputMode, TARGET_RUNNER_SHIM_SUBCOMMAND, build_rust_coverage_batch_plan,
        execute_rust_coverage_batch, load_current_population_durations,
        load_current_population_state, publish_derived_state,
    };
    assert!(!TARGET_RUNNER_SHIM_SUBCOMMAND.is_empty());
    let _ = CoverageOutputMode::SelectorEntries;

    let _ = (
        execute_rust_coverage_batch,
        build_rust_coverage_batch_plan,
        publish_derived_state,
        load_current_population_state,
        load_current_population_durations,
    );
}

fn production_rs_files(package: &str) -> Vec<PathBuf> {
    let root = crate_src_dir().join(package);
    let mut out = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with("_test.rs") {
                continue;
            }
            out.push(path);
        }
    }
    walk(&root, &mut out);
    out
}

fn file_has_forbidden_import(path: &std::path::Path, needles: &[&str]) -> Option<String> {
    let text = std::fs::read_to_string(path).unwrap();
    for needle in needles {

        if text.contains(&format!("crate::{needle}::"))
            || text.contains(&format!("use crate::{needle}"))
            || text.contains(&format!("{needle}::"))
            && text
                .lines()
                .any(|line| line.contains(needle) && (line.contains("use ") || line.contains("crate::")))
        {

            if text.contains(&format!("crate::{needle}")) {
                return Some(format!("{path:?} contains crate::{needle}"));
            }
        }
    }
    None
}

#[test]
fn production_plan_sources_do_not_import_execute_or_publish() {
    for path in production_rs_files("plan") {
        if let Some(msg) =
            file_has_forbidden_import(&path, &["execute_or_reuse", "publish_derived"])
        {
            panic!("{msg}");
        }
    }
}

#[test]
fn production_publish_sources_do_not_import_execute() {
    for path in production_rs_files("publish_derived") {
        if let Some(msg) = file_has_forbidden_import(&path, &["execute_or_reuse"]) {
            panic!("{msg}");
        }
    }
}
