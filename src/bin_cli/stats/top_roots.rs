use std::path::{Path, PathBuf};

pub(super) fn runtime_coverage_root(
    input_paths: &[String],
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
) -> PathBuf {
    let mut root = initial_runtime_root(input_paths, py_files, rs_files);
    for path in input_paths.iter().skip(1).map(PathBuf::from) {
        let path = path.canonicalize().unwrap_or(path);
        if !shrink_to_common_root(&mut root, &path) {
            return current_dir_fallback();
        }
    }
    for path in py_files.iter().chain(rs_files) {
        if !shrink_to_common_root(&mut root, path) {
            return current_dir_fallback();
        }
    }
    project_root_or(root)
}

pub(super) fn stats_runtime_py_jobs() -> Option<usize> {
    if std::env::var_os("CARGO_LLVM_COV").is_some()
        || std::env::var_os("CARGO_LLVM_COV_TARGET_DIR").is_some()
    {
        Some(1)
    } else {
        None
    }
}

fn current_dir_fallback() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn initial_runtime_root(
    input_paths: &[String],
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
) -> PathBuf {
    input_paths
        .first()
        .map(PathBuf::from)
        .and_then(|path| {
            let canonical = path.canonicalize().unwrap_or(path);
            if canonical.is_file() {
                canonical.parent().map(Path::to_path_buf)
            } else {
                Some(canonical)
            }
        })
        .or_else(|| {
            py_files
                .iter()
                .chain(rs_files)
                .next()
                .map(|path| path.parent().unwrap_or(path).to_path_buf())
        })
        .unwrap_or_else(current_dir_fallback)
}

fn shrink_to_common_root(root: &mut PathBuf, path: &Path) -> bool {
    while !path.starts_with(&root) {
        if !root.pop() {
            return false;
        }
    }
    true
}

fn project_root_or(root: PathBuf) -> PathBuf {
    let mut candidate = root.clone();
    loop {
        if candidate.join("Cargo.toml").is_file()
            || candidate.join("pyproject.toml").is_file()
            || candidate.join(".git").exists()
        {
            return candidate;
        }
        if !candidate.pop() {
            return root;
        }
    }
}
