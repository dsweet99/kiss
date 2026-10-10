use std::collections::HashMap;
use std::path::{Path, PathBuf};

use kiss::code_roles::is_default_pytest_collect_candidate;

mod config;
mod conftest;
mod fnmatch;

use config::{
    configured_python_filename_patterns, configured_python_filename_patterns_between,
    excluded_by_norecursedirs, norecursedirs_patterns_between,
};
use conftest::excluded_by_collect_ignore;
use fnmatch::fnmatch_ex;

#[cfg(test)]
mod pattern_tests;

pub(crate) fn workspace_python_collect_paths(repo_root: &Path, ignore: &[String]) -> Vec<PathBuf> {
    let tests_root = repo_root.join("tests");
    if tests_dir_is_collect_root(&tests_root, ignore) {
        return tests_root_collect_paths(repo_root, ignore, tests_root);
    }
    python_collect_candidates(repo_root, ignore)
}

fn tests_dir_is_collect_root(tests_root: &Path, ignore: &[String]) -> bool {
    tests_root.is_dir() && !kiss::path_ignored_by_prefixes("tests", ignore)
}

fn tests_root_collect_paths(
    repo_root: &Path,
    ignore: &[String],
    tests_root: PathBuf,
) -> Vec<PathBuf> {
    let mut paths = vec![tests_root.clone()];
    paths.extend(collect_candidates_outside_tests(
        repo_root,
        ignore,
        &tests_root,
    ));
    paths
}

fn collect_candidates_outside_tests(
    repo_root: &Path,
    ignore: &[String],
    tests_root: &Path,
) -> Vec<PathBuf> {
    let tests_canon = tests_root
        .canonicalize()
        .unwrap_or_else(|_| tests_root.to_path_buf());
    python_collect_candidates(repo_root, ignore)
        .into_iter()
        .filter(|path| !path_is_under_tests(path, tests_root, &tests_canon))
        .collect()
}

fn path_is_under_tests(path: &Path, tests_root: &Path, tests_canon: &Path) -> bool {
    path.starts_with(tests_canon) || path.starts_with(tests_root)
}

pub(crate) fn python_filename_selected(repo_root: &Path, path: &Path) -> bool {
    let patterns = configured_python_filename_patterns(repo_root);
    is_collect_candidate(path, patterns.as_deref())
}

pub(crate) fn python_files_under(repo_root: &Path, dir: &Path, ignore: &[String]) -> Vec<PathBuf> {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    python_collect_from(repo_root, &dir, ignore)
        .into_iter()
        .filter(|path| path.starts_with(&dir))
        .collect()
}

fn python_collect_candidates(repo_root: &Path, ignore: &[String]) -> Vec<PathBuf> {
    python_collect_from(repo_root, repo_root, ignore)
}

fn python_collect_from(repo_root: &Path, config_start: &Path, ignore: &[String]) -> Vec<PathBuf> {
    let root = repo_root.to_string_lossy().to_string();
    let (mut py_files, _rs_files) =
        kiss::gather_files_by_lang(&[root], Some(kiss::Language::Python), ignore);
    py_files.extend(symlink_python_files(repo_root, ignore));
    let patterns = configured_python_filename_patterns_between(config_start, repo_root);
    let norecursedirs = norecursedirs_patterns_between(config_start, repo_root);
    let mut conftest_ignores = HashMap::new();
    py_files
        .into_iter()
        .filter(|path| {
            is_collect_candidate(path, patterns.as_deref())
                && !excluded_by_norecursedirs(path, repo_root, &norecursedirs)
                && !excluded_by_collect_ignore(path, repo_root, &mut conftest_ignores)
        })
        .collect()
}

fn symlink_python_files(repo_root: &Path, ignore: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(repo_root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .add_custom_ignore_filename(".kissignore")
        .follow_links(false)
        .build();
    for entry in walker {
        let Ok(entry) = entry else {
            continue;
        };
        let Some(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if !ext.eq_ignore_ascii_case("py") {
            continue;
        }
        if kiss::path_skipped_by_source_ignore(path, ignore) {
            continue;
        }
        out.push(path.to_path_buf());
    }
    out
}

fn is_collect_candidate(path: &Path, patterns: Option<&[String]>) -> bool {
    match patterns {
        Some(patterns) => patterns.iter().any(|pattern| fnmatch_ex(pattern, path)),
        None => is_default_pytest_collect_candidate(path),
    }
}
