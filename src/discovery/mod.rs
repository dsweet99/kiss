use ignore::{WalkBuilder, WalkState};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    Rust,
}

impl Language {
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension().and_then(|ext| {
            if ext == "py" {
                Some(Self::Python)
            } else if ext == "rs" {
                Some(Self::Rust)
            } else {
                None
            }
        })
    }

    pub const fn extension(&self) -> &'static str {
        match self {
            Self::Python => "py",
            Self::Rust => "rs",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub language: Language,
}

pub fn find_python_files(root: &Path) -> Vec<PathBuf> {
    find_files_by_extension(root, "py")
}
pub fn find_rust_files(root: &Path) -> Vec<PathBuf> {
    find_files_by_extension(root, "rs")
}

const ALWAYS_IGNORED: &[&str] = &["__pycache__", "node_modules", ".venv", "venv", "env"];

fn has_ignored_prefix(name: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|prefix| name.starts_with(prefix))
}

fn is_always_ignored(name: &str) -> bool {
    ALWAYS_IGNORED.contains(&name)
}

fn should_ignore(path: &Path, ignore_prefixes: &[String]) -> bool {
    // Only check directory components, not the filename itself.
    let components: Vec<_> = path.components().collect();
    let dir_components = if components.len() > 1 {
        &components[..components.len() - 1]
    } else {
        return false;
    };
    dir_components.iter().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| has_ignored_prefix(s, ignore_prefixes) || is_always_ignored(s))
    })
}

pub fn find_source_files(root: &Path) -> Vec<SourceFile> {
    find_source_files_with_ignore(root, &[])
}

pub fn find_source_files_with_ignore(root: &Path, ignore_prefixes: &[String]) -> Vec<SourceFile> {
    let results = Mutex::new(Vec::new());
    WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .add_custom_ignore_filename(".kissignore")
        .build_parallel()
        .run(|| Box::new(|entry| process_source_entry(entry, ignore_prefixes, &results)));
    results.into_inner().unwrap()
}

pub fn gather_files_by_lang(
    paths: &[String],
    lang_filter: Option<Language>,
    ignore_prefixes: &[String],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut py_files, mut rs_files) = (Vec::new(), Vec::new());
    for path in paths {
        for sf in find_source_files_with_ignore(Path::new(path), ignore_prefixes) {
            let canonical = sf.path.canonicalize().unwrap_or(sf.path);
            match (sf.language, lang_filter) {
                (Language::Python, None | Some(Language::Python)) => py_files.push(canonical),
                (Language::Rust, None | Some(Language::Rust)) => rs_files.push(canonical),
                _ => {}
            }
        }
    }
    (py_files, rs_files)
}

fn process_source_entry(
    entry: Result<ignore::DirEntry, ignore::Error>,
    ignore_prefixes: &[String],
    results: &Mutex<Vec<SourceFile>>,
) -> WalkState {
    let Ok(entry) = entry else {
        return WalkState::Continue;
    };
    if !entry.file_type().is_some_and(|ft| ft.is_file()) {
        return WalkState::Continue;
    }
    let path = entry.into_path();
    if should_ignore(&path, ignore_prefixes) {
        return WalkState::Continue;
    }
    if let Some(language) = Language::from_path(&path) {
        results.lock().unwrap().push(SourceFile { path, language });
    }
    WalkState::Continue
}

fn find_files_by_extension(root: &Path, ext: &str) -> Vec<PathBuf> {
    let results = Mutex::new(Vec::new());
    WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .add_custom_ignore_filename(".kissignore")
        .build_parallel()
        .run(|| Box::new(|entry| process_ext_entry(entry, ext, &results)));
    results.into_inner().unwrap()
}

fn process_ext_entry(
    entry: Result<ignore::DirEntry, ignore::Error>,
    ext: &str,
    results: &Mutex<Vec<PathBuf>>,
) -> WalkState {
    let Ok(entry) = entry else {
        return WalkState::Continue;
    };
    if !entry.file_type().is_some_and(|ft| ft.is_file()) {
        return WalkState::Continue;
    }
    if entry.path().extension().is_some_and(|e| e == ext) {
        results.lock().unwrap().push(entry.into_path());
    }
    WalkState::Continue
}

#[cfg(test)]
#[path = "discovery_test.rs"]
mod tests;
