use super::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn vmrss_kb() -> u64 {
    let text = std::fs::read_to_string("/proc/self/status").expect("status");
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest
                .split_whitespace()
                .next()
                .expect("rss value")
                .parse()
                .expect("rss parse");
        }
    }
    panic!("VmRSS missing");
}

fn plan_once(
    root: &Path,
    sources: &[PathBuf],
    ignore: &[String],
    lang_filter: Option<kiss::Language>,
) {
    let empty_lines = BTreeMap::new();
    let empty: [String; 0] = [];
    combined_selectors_with_direct(CombinedSelectorInput {
        repo_root: root,
        source_paths: sources,
        test_paths: &[],
        changed_lines: &empty_lines,
        test_args: crate::test_runner::language_keyed::LanguageKeyed {
            python: &empty,
            rust: &empty,
        },
        lang_filter,
        ignore,
        extra_direct_python: &[],
        extra_direct_rust: &[],
        include_prior_failures: false,
    })
    .expect("covering_select");
}

fn rust_selectors_once(root: &Path, ignore: &[String]) -> Vec<String> {
    if let Some(cached) =
        crate::test_runner::workspace_selector_cache::load_cached_rust_workspace_selectors(
            root, ignore,
        )
    {
        return cached;
    }
    let ids = crate::test_runner::runners::enumerate_workspace_rust_selectors(root, ignore)
        .expect("enumerate rust selectors");
    crate::test_runner::workspace_selector_cache::store_rust_workspace_selectors(
        root, ignore, &ids,
    );
    ids
}

#[test]
fn covering_select_repeat_does_not_grow_rss() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ignore = vec!["src/main.rs".to_string()];
    let py = root.join("tests").join("test_cli_shims.py");
    let orig = std::fs::read(&py).expect("test_cli_shims.py");
    struct Restore(PathBuf, Vec<u8>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::fs::write(&self.0, &self.1);
        }
    }
    let _restore = Restore(py.clone(), orig.clone());
    let warmup = rust_selectors_once(&root, &ignore);
    let start = vmrss_kb();
    for i in 0..4 {
        let mut next = orig.clone();
        next.extend_from_slice(format!("\n# rss-touch-{i}\n").as_bytes());
        std::fs::write(&py, &next).expect("touch python");
        let again = rust_selectors_once(&root, &ignore);
        assert_eq!(again.len(), warmup.len());
    }
    let grew = vmrss_kb().saturating_sub(start);
    assert!(
        grew < 8192,
        "rust selectors after python mtime RSS grew {grew} kB over 4 repeats (start {start} kB)"
    );
}

#[test]
fn covering_select_python_only_repeat_does_not_grow_rss() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ignore = vec!["src/main.rs".to_string()];
    let root_s = root.to_string_lossy().into_owned();
    let (py, rs) = kiss::gather_files_by_lang(std::slice::from_ref(&root_s), None, &ignore);
    let mut sources = py;
    sources.extend(rs);
    plan_once(&root, &sources, &ignore, Some(kiss::Language::Python));
    let start = vmrss_kb();
    for _ in 0..4 {
        plan_once(&root, &sources, &ignore, Some(kiss::Language::Python));
    }
    let grew = vmrss_kb().saturating_sub(start);
    assert!(
        grew < 8192,
        "python-only covering_select RSS grew {grew} kB over 4 repeats (start {start} kB)"
    );
}

#[test]
fn load_population_repeat_does_not_grow_rss() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let _ = crate::test_runner::rust_coverage_index::load_current_rust_population_state(
        &root,
        None,
        &[],
    );
    let start = vmrss_kb();
    for _ in 0..4 {
        let _ = crate::test_runner::rust_coverage_index::load_current_rust_population_state(
            &root,
            None,
            &[],
        );
    }
    let grew = vmrss_kb().saturating_sub(start);
    assert!(
        grew < 8192,
        "load_current_population RSS grew {grew} kB over 4 repeats (start {start} kB)"
    );
}
