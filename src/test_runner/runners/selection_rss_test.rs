use super::*;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use crate::test_runner::test_mode_fixtures::clone_warm_committed_repo;

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
    let empty: [String; 0] = [];
    combined_selectors_with_direct(CombinedSelectorInput {
        repo_root: root,
        source_paths: sources,
        test_paths: &[],
        test_args: crate::test_runner::language_keyed::LanguageKeyed {
            python: &empty,
            rust: &empty,
        },
        lang_filter,
        ignore,
        extra_direct: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        include_prior_failures: false,
    })
    .expect("selection");
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
fn selection_repeat_does_not_grow_rss() {
    let tmp = TempDir::new().expect("rss mini repo");
    let root = tmp.path();
    let _lib = clone_warm_committed_repo(root);
    let py = root.join("touch.py");
    std::fs::write(&py, "# seed\n").expect("touch.py");
    let ignore = vec!["touch.py".to_string()];
    let orig = std::fs::read(&py).expect("touch.py");
    struct Restore(PathBuf, Vec<u8>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::fs::write(&self.0, &self.1);
        }
    }
    let _restore = Restore(py.clone(), orig.clone());
    let warmup = rust_selectors_once(root, &ignore);
    let start = vmrss_kb();
    for i in 0..2 {
        let mut next = orig.clone();
        next.extend_from_slice(format!("\n# rss-touch-{i}\n").as_bytes());
        std::fs::write(&py, &next).expect("touch python");
        let again = rust_selectors_once(root, &ignore);
        assert_eq!(again.len(), warmup.len());
    }
    let grew = vmrss_kb().saturating_sub(start);
    assert!(
        grew < 8192,
        "rust selectors after python mtime RSS grew {grew} kB over 2 repeats (start {start} kB)"
    );
}

#[test]
fn selection_python_only_repeat_does_not_grow_rss() {
    let tmp = TempDir::new().expect("python rss mini repo");
    let root = tmp.path();
    let _lib = clone_warm_committed_repo(root);
    let py = root.join("app.py");
    std::fs::write(&py, "def f():\n    return 1\n").expect("app.py");
    let ignore: [String; 0] = [];
    let root_s = root.to_string_lossy().into_owned();
    let (py_files, rs) = kiss::gather_files_by_lang(std::slice::from_ref(&root_s), None, &ignore);
    let mut sources = py_files;
    sources.extend(rs);
    plan_once(root, &sources, &ignore, Some(kiss::Language::Python));
    let start = vmrss_kb();
    for _ in 0..2 {
        plan_once(root, &sources, &ignore, Some(kiss::Language::Python));
    }
    let grew = vmrss_kb().saturating_sub(start);
    assert!(
        grew < 8192,
        "python-only selection RSS grew {grew} kB over 2 repeats (start {start} kB)"
    );
}
