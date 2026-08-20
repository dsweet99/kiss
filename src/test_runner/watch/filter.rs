use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use kiss::Language;

use crate::bin_cli::args::TestInvocation;

const HARD_EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".kiss",
    "target",
    ".pytest_cache",
    ".rslip_cache",
    "__pycache__",
    ".venv",
    "venv",
    "node_modules",
];

pub(crate) struct WatchPathFilter {
    repo_root: PathBuf,
    gitignore: Gitignore,
    cli_ignore: Vec<String>,
    lang_filter: Option<Language>,
    invocation: TestInvocation,
    exact_files: Option<Vec<PathBuf>>,
}

impl WatchPathFilter {
    pub(crate) fn build(
        repo_root: &Path,
        cli_ignore: &[String],
        lang_filter: Option<Language>,
        invocation: &TestInvocation,
    ) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            gitignore: build_gitignore(repo_root),
            cli_ignore: kiss::normalize_ignore_prefixes(cli_ignore),
            lang_filter,
            invocation: invocation.clone(),
            exact_files: exact_file_targets(invocation),
        }
    }

    pub(crate) fn cli_ignore(&self) -> &[String] {
        &self.cli_ignore
    }

    pub(crate) fn lang_filter(&self) -> Option<Language> {
        self.lang_filter
    }

    pub(crate) fn invocation(&self) -> &TestInvocation {
        &self.invocation
    }

    pub(crate) fn is_ignore_file(&self, rel: &Path) -> bool {
        rel == Path::new(".gitignore")
            || rel == Path::new(".kissignore")
            || rel == Path::new(".git/info/exclude")
            || matches!(
                rel.file_name().and_then(|n| n.to_str()),
                Some(".gitignore" | ".kissignore")
            )
    }

    #[cfg(test)]
    pub(crate) fn is_kissconfig_file(&self, rel: &Path) -> bool {
        is_kissconfig_path(rel)
    }

    pub(crate) fn is_relevant(&self, rel: &Path) -> bool {
        if is_kissconfig_path(rel) {
            return true;
        }
        if is_hard_excluded(rel) {
            return is_git_support_path(rel, &self.invocation);
        }
        if component_has_cli_prefix(rel, &self.cli_ignore) {
            return false;
        }
        let abs = self.repo_root.join(rel);
        if self.gitignore.matched(&abs, abs.is_dir()).is_ignore() {
            return false;
        }
        self.is_support_or_source(rel)
    }

    fn is_support_or_source(&self, rel: &Path) -> bool {
        if self.is_ignore_file(rel)
            || is_kissconfig_path(rel)
            || is_git_support_path(rel, &self.invocation)
            || is_support_input(rel)
        {
            return true;
        }
        if let Some(files) = &self.exact_files {
            return files.iter().any(|f| f == rel);
        }
        matches_lang_filter(rel, self.lang_filter)
    }
}

fn exact_file_targets(invocation: &TestInvocation) -> Option<Vec<PathBuf>> {
    let TestInvocation::Targets(targets) = invocation else {
        return None;
    };
    let files: Vec<_> = targets
        .iter()
        .map(|raw| raw.split_once("::").map_or(raw.as_str(), |(p, _)| p))
        .filter(|path_part| {
            Path::new(path_part)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("py") || e.eq_ignore_ascii_case("rs"))
        })
        .map(PathBuf::from)
        .collect();
    if files.is_empty() { None } else { Some(files) }
}

fn matches_lang_filter(rel: &Path, lang_filter: Option<Language>) -> bool {
    let is_py = rel
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("py"));
    let is_rs = kiss::Language::is_rust_path(rel)
        || rel
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("rs"));
    match lang_filter {
        Some(Language::Python) => is_py,
        Some(Language::Rust) => is_rs,
        None => is_py || is_rs,
    }
}

fn build_gitignore(repo_root: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(repo_root);
    let _ = builder.add(repo_root.join(".gitignore"));
    let _ = builder.add(repo_root.join(".git/info/exclude"));
    let _ = builder.add(repo_root.join(".kissignore"));
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

fn is_hard_excluded(rel: &Path) -> bool {
    rel.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|name| HARD_EXCLUDED_DIRS.contains(&name))
    })
}

fn component_has_cli_prefix(rel: &Path, prefixes: &[String]) -> bool {
    for component in rel.components() {
        let Some(name) = component.as_os_str().to_str() else {
            continue;
        };
        if prefixes.iter().any(|prefix| name.starts_with(prefix)) {
            return true;
        }
    }
    false
}

fn is_git_support_path(rel: &Path, invocation: &TestInvocation) -> bool {
    if rel == Path::new(".git/info/exclude") {
        return true;
    }
    if !matches!(
        invocation,
        TestInvocation::Commit | TestInvocation::Base | TestInvocation::Main
    ) {
        return false;
    }
    if rel == Path::new(".git/HEAD") || rel == Path::new(".git/index") {
        return true;
    }
    matches!(invocation, TestInvocation::Base | TestInvocation::Main)
        && (rel.starts_with(".git/refs/heads") || rel == Path::new(".git/packed-refs"))
}

fn is_kissconfig_path(rel: &Path) -> bool {
    rel == Path::new(".kissconfig")
}

fn is_support_input(rel: &Path) -> bool {
    if rel.file_name().and_then(|n| n.to_str()) == Some("conftest.py") {
        return true;
    }

    if is_source_ext(rel) {
        return false;
    }
    rslip::is_rslip_cache_input(rel) || rust_llvm_cov_runner::is_rust_cov_cache_input(rel)
}

fn is_source_ext(rel: &Path) -> bool {
    rel.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("rs"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_kiss_and_target() {
        let tmp = tempfile::tempdir().unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &TestInvocation::All);
        assert!(!f.is_relevant(Path::new(".kiss/cache")));
        assert!(!f.is_relevant(Path::new("target/debug/foo")));
        assert!(f.is_relevant(Path::new("src/lib.rs")));
        assert!(f.is_relevant(Path::new("pkg/mod.py")));
    }

    #[test]
    fn kissconfig_is_relevant_support_file() {
        let tmp = tempfile::tempdir().unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &TestInvocation::All);
        assert!(f.is_kissconfig_file(Path::new(".kissconfig")));
        assert!(f.is_relevant(Path::new(".kissconfig")));
        assert!(!f.is_kissconfig_file(Path::new("nested/.kissconfig")));
        assert!(!f.is_relevant(Path::new("nested/.kissconfig")));
    }

    #[test]
    fn gitignored_kissconfig_is_still_relevant() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), ".kissconfig\n").unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &TestInvocation::All);
        assert!(
            f.is_relevant(Path::new(".kissconfig")),
            "H2: gitignored .kissconfig must remain watch-relevant"
        );
    }

    #[test]
    fn support_inputs_reuse_cache_helpers() {
        let tmp = tempfile::tempdir().unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &TestInvocation::All);
        assert!(f.is_relevant(Path::new("pytest.ini")));
        assert!(f.is_relevant(Path::new("Cargo.toml")));
        assert!(f.is_relevant(Path::new("rust-toolchain.toml")));
        assert!(f.is_relevant(Path::new(".cargo/config.toml")));
        assert!(f.is_relevant(Path::new("foo.inc")));
        assert!(f.is_relevant(Path::new("conftest.py")));

        let exact = WatchPathFilter::build(
            tmp.path(),
            &[],
            None,
            &TestInvocation::Targets(vec!["src/a.py".into()]),
        );
        assert!(exact.is_relevant(Path::new("src/a.py")));
        assert!(!exact.is_relevant(Path::new("src/b.py")));
        assert!(exact.is_relevant(Path::new("pytest.ini")));
    }

    #[test]
    fn basename_exclude_is_not_an_ignore_support_file() {
        let tmp = tempfile::tempdir().unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &TestInvocation::All);
        assert!(!f.is_ignore_file(Path::new("vendor/exclude")));
        assert!(!f.is_ignore_file(Path::new("exclude")));
        assert!(f.is_ignore_file(Path::new(".git/info/exclude")));
        assert!(f.is_ignore_file(Path::new(".gitignore")));
        assert!(f.is_ignore_file(Path::new("nested/.gitignore")));
    }
}
