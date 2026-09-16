use std::path::{Path, PathBuf};

use kiss::code_roles::is_default_pytest_collect_candidate;

pub(crate) fn workspace_python_collect_paths(
    repo_root: &Path,
    ignore: &[String],
) -> Vec<PathBuf> {
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

fn python_collect_candidates(repo_root: &Path, ignore: &[String]) -> Vec<PathBuf> {
    let root = repo_root.to_string_lossy().to_string();
    let (py_files, _rs_files) =
        kiss::gather_files_by_lang(&[root], Some(kiss::Language::Python), ignore);
    py_files
        .into_iter()
        .filter(|path| is_default_pytest_collect_candidate(path))
        .collect()
}
