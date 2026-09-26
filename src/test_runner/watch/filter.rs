use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use kiss::Language;

use super::roots::{is_source_file_operand, resolve_target_abs};
use crate::test_runner::target_request::{GitFocus, TargetFocus, TargetRequest};
#[cfg(test)]
use crate::test_runner::target_request::{operands_request, request_from_focus, workspace_request};

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
    request: TargetRequest,
    target_scope: Option<WatchTargetScope>,
    watched_config: PathBuf,
    /// Repo-relative Rust paths reached only via `include!` / `#[path]` / conventional
    /// `mod` expansion (same basis as worktree extras). Gitignored members still wake watch.
    expand_extras: HashSet<PathBuf>,
}

struct WatchTargetScope {
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

impl WatchTargetScope {
    fn contains(&self, rel: &Path) -> bool {
        self.files.iter().any(|file| file == rel)
            || self
                .dirs
                .iter()
                .any(|dir| rel == dir || rel.starts_with(dir))
    }
}

impl WatchPathFilter {
    #[cfg(test)]
    pub(crate) fn build(
        repo_root: &Path,
        cli_ignore: &[String],
        lang_filter: Option<Language>,
        request: &TargetRequest,
    ) -> Self {
        Self::build_with_config(
            repo_root,
            cli_ignore,
            lang_filter,
            request,
            Path::new(".kissconfig"),
        )
    }

    pub(crate) fn build_with_config(
        repo_root: &Path,
        cli_ignore: &[String],
        lang_filter: Option<Language>,
        request: &TargetRequest,
        config_path: &Path,
    ) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            gitignore: build_gitignore(repo_root),
            cli_ignore: kiss::normalize_ignore_prefixes(cli_ignore),
            lang_filter,
            request: request.clone(),
            target_scope: target_scope(repo_root, &request.focus),
            watched_config: config_rel_for_watch(repo_root, config_path),
            expand_extras: rust_expand_extras(repo_root),
        }
    }

    pub(crate) fn rebuild(&self) -> Self {
        Self::build_with_config(
            &self.repo_root,
            &self.cli_ignore,
            self.lang_filter,
            &self.request,
            &self.watched_config,
        )
    }

    /// Refresh expand-extras after a settle so newly declared gitignored targets wake later.
    pub(crate) fn refresh_expand_extras(&mut self) {
        self.expand_extras = rust_expand_extras(&self.repo_root);
    }

    pub(crate) fn is_ignore_file(&self, rel: &Path) -> bool {
        is_watch_ignore_file(rel)
    }

    #[cfg(test)]
    pub(crate) fn is_kissconfig_file(&self, rel: &Path) -> bool {
        self.is_watched_config(rel)
    }

    pub(crate) fn is_relevant(&self, rel: &Path) -> bool {
        if self.is_watched_config(rel) {
            return true;
        }
        if is_hard_excluded(rel) {
            return is_git_support_path(rel, &self.request.focus);
        }
        if kiss::path_ignored_by_prefixes(&rel.to_string_lossy(), &self.cli_ignore) {
            // CLI-ignored expand extras / support still feed ITE/worktree (gather /
            // rust_full with empty ignore). Discovered (non-gitignored) Rust sources
            // also feed empty-ignore rust_full — wake so ensure reconsults (#11/#12).
            // Non-test Python source inputs feed python_source_input_fingerprint the
            // same way (#13). CLI-ignored test modules (excluded from that fingerprint)
            // stay denied.
            if self.is_expand_extra(rel) || is_support_input(rel) {
                return true;
            }
            let abs = self.repo_root.join(rel);
            if self.gitignore.matched(&abs, abs.is_dir()).is_ignore() {
                return false;
            }
            if kiss::Language::is_rust_path(rel)
                || rel
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("rs"))
            {
                return self.is_support_or_source(rel);
            }
            if kiss::rslip::is_rslip_cache_input(rel) && !kiss::is_python_test_module_path(rel) {
                return self.is_support_or_source(rel);
            }
            return false;
        }
        let abs = self.repo_root.join(rel);
        if self.gitignore.matched(&abs, abs.is_dir()).is_ignore() {
            // Gitignored expand extras feed ITE/worktree; support fragments (.inc, …) may
            // also be gitignored. Non-test Python source inputs feed
            // python_source_input_fingerprint with no gitignore consult (#14) — wake so
            // ensure reconsults. Gitignored test modules stay denied. Unrelated noise denied.
            if self.is_expand_extra(rel) || is_support_input(rel) {
                return true;
            }
            if kiss::rslip::is_rslip_cache_input(rel) && !kiss::is_python_test_module_path(rel) {
                return self.is_support_or_source(rel);
            }
            return false;
        }
        self.is_support_or_source(rel)
    }

    fn is_expand_extra(&self, rel: &Path) -> bool {
        let normalized: PathBuf = rel.components().collect();
        self.expand_extras.contains(rel) || self.expand_extras.contains(&normalized)
    }

    fn is_watched_config(&self, rel: &Path) -> bool {
        rel == self.watched_config.as_path()
    }

    fn is_support_or_source(&self, rel: &Path) -> bool {
        if self.is_ignore_file(rel)
            || self.is_watched_config(rel)
            || is_git_support_path(rel, &self.request.focus)
            || is_support_input(rel)
        {
            return true;
        }
        if let Some(scope) = &self.target_scope {
            return matches_lang_filter(rel, self.lang_filter) && scope.contains(rel);
        }
        matches_lang_filter(rel, self.lang_filter)
    }
}

fn target_scope(repo_root: &Path, focus: &TargetFocus) -> Option<WatchTargetScope> {
    let TargetFocus::Operands(operands) = focus else {
        return None;
    };
    let mut scope = WatchTargetScope {
        files: Vec::new(),
        dirs: Vec::new(),
    };
    for raw in operands.iter().map(|operand| operand.raw.as_str()) {
        let path_part = raw.split_once("::").map_or(raw, |(path, _)| path);
        let absolute = resolve_target_abs(repo_root, Path::new(path_part));
        let path = normalize_target_path(repo_root, &absolute);
        if is_source_file_operand(Path::new(path_part), &absolute) {
            scope.files.push(path);
        } else {
            scope.dirs.push(path);
        }
    }
    Some(scope)
}

fn normalize_target_path(repo_root: &Path, absolute: &Path) -> PathBuf {
    absolute
        .strip_prefix(repo_root)
        .unwrap_or(absolute)
        .components()
        .collect()
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

/// Repo-relative paths that `expand_rust_files` reaches beyond walk/git discovery
/// (same extras set folded into the worktree token).
fn rust_expand_extras(repo_root: &Path) -> HashSet<PathBuf> {
    let root = repo_root.to_string_lossy().into_owned();
    let (_, discovered) = kiss::gather_files_by_lang_opts(
        std::slice::from_ref(&root),
        Some(kiss::Language::Rust),
        &[],
        false,
    );
    let expanded = kiss::expand_rust_files(discovered.clone());
    let baseline: HashSet<_> = discovered.into_iter().collect();
    let mut extras = HashSet::new();
    for path in expanded {
        if baseline.contains(&path) {
            continue;
        }
        let rel = path
            .strip_prefix(repo_root)
            .unwrap_or(path.as_path())
            .components()
            .collect();
        extras.insert(rel);
    }
    extras
}

pub(crate) fn is_hard_excluded(rel: &Path) -> bool {
    rel.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|name| HARD_EXCLUDED_DIRS.contains(&name))
    })
}

pub(crate) fn is_git_support_path(rel: &Path, focus: &TargetFocus) -> bool {
    if rel == Path::new(".git/info/exclude") {
        return true;
    }
    let TargetFocus::Git(git) = focus else {
        return false;
    };
    if rel == Path::new(".git/HEAD") || rel == Path::new(".git/index") {
        return true;
    }
    !matches!(git, GitFocus::Commit)
        && (rel.starts_with(".git/refs/heads") || rel == Path::new(".git/packed-refs"))
}

pub(crate) fn is_watch_ignore_file(rel: &Path) -> bool {
    rel == Path::new(".gitignore")
        || rel == Path::new(".kissignore")
        || rel == Path::new(".git/info/exclude")
        || matches!(
            rel.file_name().and_then(|n| n.to_str()),
            Some(".gitignore" | ".kissignore")
        )
}

pub(crate) fn path_should_enter_watch_queue(
    rel: &Path,
    focus: &TargetFocus,
    watched_config: &Path,
) -> bool {
    if is_git_support_path(rel, focus) || rel == watched_config {
        return true;
    }
    if is_hard_excluded(rel) {
        return false;
    }
    is_watch_ignore_file(rel) || is_support_input(rel) || matches_lang_filter(rel, None)
}

pub(crate) fn config_rel_for_watch(repo_root: &Path, config_path: &Path) -> PathBuf {
    if config_path.as_os_str().is_empty() {
        return PathBuf::from(".kissconfig");
    }
    let abs = resolve_target_abs(repo_root, config_path);
    abs.strip_prefix(repo_root)
        .map(Path::to_path_buf)
        .unwrap_or(abs)
}

fn is_support_input(rel: &Path) -> bool {
    if rel.file_name().and_then(|n| n.to_str()) == Some("conftest.py") {
        return true;
    }

    if is_source_ext(rel) {
        return false;
    }
    kiss::rslip::is_rslip_cache_input(rel)
        || kiss::rust_llvm_cov_runner::is_rust_cov_cache_input(rel)
}

fn is_source_ext(rel: &Path) -> bool {
    rel.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("rs"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operands(raws: &[&str]) -> TargetRequest {
        operands_request(
            &raws
                .iter()
                .map(|raw| (*raw).to_string())
                .collect::<Vec<_>>(),
            None,
            &[],
        )
    }

    fn commit_request() -> TargetRequest {
        request_from_focus(TargetFocus::Git(GitFocus::Commit), None, &[])
    }

    #[test]
    fn filter_operands_helper_uses_operands_request() {
        assert_eq!(
            operands(&["z.py", "a.py"]),
            operands_request(&["z.py".into(), "a.py".into()], None, &[])
        );
    }

    #[test]
    fn excludes_kiss_and_target() {
        let tmp = tempfile::tempdir().unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &workspace_request(None, &[]));
        assert!(!f.is_relevant(Path::new(".kiss/cache")));
        assert!(!f.is_relevant(Path::new("target/debug/foo")));
        assert!(f.is_relevant(Path::new("src/lib.rs")));
        assert!(f.is_relevant(Path::new("pkg/mod.py")));
    }

    #[test]
    fn kissconfig_is_relevant_support_file() {
        let tmp = tempfile::tempdir().unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &workspace_request(None, &[]));
        assert!(f.is_kissconfig_file(Path::new(".kissconfig")));
        assert!(f.is_relevant(Path::new(".kissconfig")));
        assert!(!f.is_kissconfig_file(Path::new("nested/.kissconfig")));
        assert!(!f.is_relevant(Path::new("nested/.kissconfig")));
    }

    #[test]
    fn config_override_file_is_watch_relevant() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = workspace_request(None, &[]);
        let f = WatchPathFilter::build_with_config(
            tmp.path(),
            &[],
            None,
            &workspace,
            Path::new("custom.toml"),
        );
        assert!(f.is_kissconfig_file(Path::new("custom.toml")));
        assert!(f.is_relevant(Path::new("custom.toml")));
        assert!(!f.is_kissconfig_file(Path::new(".kissconfig")));
        assert!(!f.is_relevant(Path::new(".kissconfig")));

        std::fs::write(tmp.path().join(".gitignore"), "custom.toml\n").unwrap();
        let ignored = WatchPathFilter::build_with_config(
            tmp.path(),
            &[],
            None,
            &workspace,
            Path::new("custom.toml"),
        );
        assert!(
            ignored.is_relevant(Path::new("custom.toml")),
            "--config FILE must remain watch-relevant when gitignored"
        );
    }

    #[test]
    fn gitignored_kissconfig_is_still_relevant() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), ".kissconfig\n").unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &workspace_request(None, &[]));
        assert!(
            f.is_relevant(Path::new(".kissconfig")),
            "H2: gitignored .kissconfig must remain watch-relevant"
        );
    }

    #[test]
    fn gitignored_rust_expand_targets_are_watch_relevant() {
        // kt_bug.md class #10: --watch must wake on gitignored expand extras
        // (conventional / #[path] / include!), not only on watched config.
        // Needs a real git repo so discovery's WalkBuilder applies .gitignore
        // (same premise as worktree expand-extras fingerprint).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["init", "-q", "-b", "main"])
                .status()
                .unwrap()
                .success()
        );
        for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
            assert!(
                kiss::scrubbed_git_command(root)
                    .args(["config", kv.0, kv.1])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(
            root.join(".gitignore"),
            "hidden.rs\nalt.rs\ngen.rs\nnoise.bin\n",
        )
        .unwrap();
        std::fs::write(
            root.join("lib.rs"),
            concat!(
                "mod hidden;\n",
                "#[path = \"alt.rs\"]\n",
                "mod via_path;\n",
                "include!(\"gen.rs\");\n",
            ),
        )
        .unwrap();
        std::fs::write(root.join("hidden.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(root.join("alt.rs"), "pub fn b() {}\n").unwrap();
        std::fs::write(root.join("gen.rs"), "fn c() {}\n").unwrap();
        std::fs::write(root.join("noise.bin"), "x").unwrap();
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["add", "-A"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["commit", "-m", "init"])
                .status()
                .unwrap()
                .success()
        );

        let expanded = kiss::expand_rust_files(vec![root.join("lib.rs")]);
        assert!(
            expanded.iter().any(|p| p.ends_with("hidden.rs")),
            "expand must reach conventional mod target"
        );
        assert!(
            expanded.iter().any(|p| p.ends_with("alt.rs")),
            "expand must reach #[path] target"
        );
        assert!(
            expanded.iter().any(|p| p.ends_with("gen.rs")),
            "expand must reach include! target"
        );

        let f = WatchPathFilter::build(root, &[], None, &workspace_request(None, &[]));
        assert!(
            f.is_relevant(Path::new("hidden.rs")),
            "gitignored conventional mod expand target must be watch-relevant"
        );
        assert!(
            f.is_relevant(Path::new("alt.rs")),
            "gitignored #[path] expand target must be watch-relevant"
        );
        assert!(
            f.is_relevant(Path::new("gen.rs")),
            "gitignored include! expand target must be watch-relevant"
        );
        assert!(
            f.is_relevant(Path::new("lib.rs")),
            "control: non-gitignored source stays relevant"
        );
        assert!(
            !f.is_relevant(Path::new("noise.bin")),
            "unrelated gitignored noise must stay denied"
        );
    }

    #[test]
    fn cli_ignored_rust_expand_targets_are_watch_relevant() {
        // kt_bug.md class #11: same expand-extra fixture as #10, plus cli_ignore
        // listing those extras. Worktree still digests them (gather with empty
        // ignore); watch must wake. Unrelated cli_ignored noise stays denied.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["init", "-q", "-b", "main"])
                .status()
                .unwrap()
                .success()
        );
        for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
            assert!(
                kiss::scrubbed_git_command(root)
                    .args(["config", kv.0, kv.1])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(
            root.join(".gitignore"),
            "hidden.rs\nalt.rs\ngen.rs\nnoise.rs\n",
        )
        .unwrap();
        std::fs::write(
            root.join("lib.rs"),
            concat!(
                "mod hidden;\n",
                "#[path = \"alt.rs\"]\n",
                "mod via_path;\n",
                "include!(\"gen.rs\");\n",
            ),
        )
        .unwrap();
        std::fs::write(root.join("hidden.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(root.join("alt.rs"), "pub fn b() {}\n").unwrap();
        std::fs::write(root.join("gen.rs"), "fn c() {}\n").unwrap();
        std::fs::write(root.join("noise.rs"), "pub fn n() {}\n").unwrap();
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["add", "-A"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["commit", "-m", "init"])
                .status()
                .unwrap()
                .success()
        );

        let expanded = kiss::expand_rust_files(vec![root.join("lib.rs")]);
        assert!(
            expanded.iter().any(|p| p.ends_with("hidden.rs")),
            "expand must reach conventional mod target"
        );
        assert!(
            expanded.iter().any(|p| p.ends_with("alt.rs")),
            "expand must reach #[path] target"
        );
        assert!(
            expanded.iter().any(|p| p.ends_with("gen.rs")),
            "expand must reach include! target"
        );

        let cli_ignore = vec![
            "hidden.rs".into(),
            "alt.rs".into(),
            "gen.rs".into(),
            "noise.rs".into(),
        ];
        let f = WatchPathFilter::build(root, &cli_ignore, None, &workspace_request(None, &[]));
        assert!(
            f.is_relevant(Path::new("hidden.rs")),
            "cli_ignored conventional mod expand target must be watch-relevant"
        );
        assert!(
            f.is_relevant(Path::new("alt.rs")),
            "cli_ignored #[path] expand target must be watch-relevant"
        );
        assert!(
            f.is_relevant(Path::new("gen.rs")),
            "cli_ignored include! expand target must be watch-relevant"
        );
        assert!(
            f.is_relevant(Path::new("lib.rs")),
            "control: non-ignored source stays relevant"
        );
        assert!(
            !f.is_relevant(Path::new("noise.rs")),
            "unrelated cli_ignored noise that is not an expand-extra must stay denied"
        );
    }

    #[test]
    fn cli_ignored_discovered_rust_sources_are_watch_relevant() {
        // kt_bug.md class #12: tracked discovered source under cli_ignore still
        // feeds capture_worktree_token via rust_full_source_fingerprint(repo, &[]).
        // Watch must wake (semantic A); gitignored non-extra noise stays denied.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["init", "-q", "-b", "main"])
                .status()
                .unwrap()
                .success()
        );
        for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
            assert!(
                kiss::scrubbed_git_command(root)
                    .args(["config", kv.0, kv.1])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(root.join(".gitignore"), "noise.rs\n").unwrap();
        std::fs::write(root.join("noise.rs"), "pub fn n() {}\n").unwrap();
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["add", "-A"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["commit", "-m", "init"])
                .status()
                .unwrap()
                .success()
        );

        let before = crate::test_runner::workspace_selector_cache::rust_full_source_fingerprint(
            root, &[],
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn a() { /* edited */ }\n").unwrap();
        let after = crate::test_runner::workspace_selector_cache::rust_full_source_fingerprint(
            root, &[],
        )
        .unwrap();
        assert_ne!(
            before, after,
            "empty-ignore rust_full must move when tracked discovered source changes"
        );

        let cli_ignore = vec!["src/lib.rs".into(), "noise.rs".into()];
        let f = WatchPathFilter::build(root, &cli_ignore, None, &workspace_request(None, &[]));
        assert!(
            f.is_relevant(Path::new("src/lib.rs")),
            "cli_ignored discovered source that feeds empty-ignore worktree must be watch-relevant"
        );
        assert!(
            !f.is_relevant(Path::new("noise.rs")),
            "gitignored cli_ignored noise that is not an expand-extra must stay denied"
        );
    }

    #[test]
    fn cli_ignored_python_sources_are_watch_relevant() {
        // kt_bug.md class #13: tracked non-test Python under cli_ignore still feeds
        // capture_worktree_token via python_source_input_fingerprint. Watch must wake
        // (semantic A). CLI-ignored test modules (excluded from that fingerprint) stay denied.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["init", "-q", "-b", "main"])
                .status()
                .unwrap()
                .success()
        );
        for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
            assert!(
                kiss::scrubbed_git_command(root)
                    .args(["config", kv.0, kv.1])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::fs::write(root.join("pkg/mod.py"), "x = 1\n").unwrap();
        std::fs::write(root.join("test_x.py"), "def test_ok():\n    assert True\n").unwrap();
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["add", "-A"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["commit", "-m", "init"])
                .status()
                .unwrap()
                .success()
        );

        let before =
            crate::test_runner::python_coverage_index::storage::python_source_input_fingerprint(
                root,
            )
            .unwrap();
        std::fs::write(root.join("pkg/mod.py"), "x = 2\n").unwrap();
        let after =
            crate::test_runner::python_coverage_index::storage::python_source_input_fingerprint(
                root,
            )
            .unwrap();
        assert_ne!(
            before, after,
            "python_source_input_fingerprint must move when non-test source changes"
        );

        let cli_ignore = vec!["pkg/mod.py".into(), "test_x.py".into()];
        let f = WatchPathFilter::build(root, &cli_ignore, None, &workspace_request(None, &[]));
        assert!(
            f.is_relevant(Path::new("pkg/mod.py")),
            "cli_ignored non-test Python that feeds worktree python fingerprint must be watch-relevant"
        );
        assert!(
            !f.is_relevant(Path::new("test_x.py")),
            "cli_ignored Python test module (excluded from fingerprint) must stay denied"
        );
    }

    #[test]
    fn gitignored_python_sources_are_watch_relevant() {
        // kt_bug.md class #14: gitignored non-test Python still feeds
        // capture_worktree_token via python_source_input_fingerprint. Watch must wake
        // (semantic A). Gitignored test modules (excluded from that fingerprint) stay denied.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["init", "-q", "-b", "main"])
                .status()
                .unwrap()
                .success()
        );
        for kv in [("user.email", "t@t.t"), ("user.name", "t")] {
            assert!(
                kiss::scrubbed_git_command(root)
                    .args(["config", kv.0, kv.1])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::fs::write(
            root.join(".gitignore"),
            "hidden_lib.py\npkg/secret.py\ntest_ignored.py\n",
        )
        .unwrap();
        std::fs::write(root.join("app.py"), "x = 0\n").unwrap();
        std::fs::write(root.join("hidden_lib.py"), "x = 1\n").unwrap();
        std::fs::write(root.join("pkg/secret.py"), "y = 1\n").unwrap();
        std::fs::write(
            root.join("test_ignored.py"),
            "def test_ok():\n    assert True\n",
        )
        .unwrap();
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["add", "-A"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            kiss::scrubbed_git_command(root)
                .args(["commit", "-m", "init"])
                .status()
                .unwrap()
                .success()
        );

        let fp = |root: &Path| {
            crate::test_runner::python_coverage_index::storage::python_source_input_fingerprint(
                root,
            )
            .unwrap()
        };
        let base = fp(root);
        std::fs::write(root.join("hidden_lib.py"), "x = 2\n").unwrap();
        assert_ne!(
            base,
            fp(root),
            "fingerprint must move when gitignored hidden_lib.py changes"
        );
        std::fs::write(root.join("hidden_lib.py"), "x = 1\n").unwrap();
        let base = fp(root);
        std::fs::write(root.join("pkg/secret.py"), "y = 2\n").unwrap();
        assert_ne!(
            base,
            fp(root),
            "fingerprint must move when gitignored pkg/secret.py changes"
        );

        let f = WatchPathFilter::build(root, &[], None, &workspace_request(None, &[]));
        assert!(
            f.is_relevant(Path::new("hidden_lib.py")),
            "gitignored non-test Python that feeds worktree python fingerprint must be watch-relevant"
        );
        assert!(
            f.is_relevant(Path::new("pkg/secret.py")),
            "gitignored path-pattern non-test Python that feeds fingerprint must be watch-relevant"
        );
        assert!(
            !f.is_relevant(Path::new("test_ignored.py")),
            "gitignored Python test module (excluded from fingerprint) must stay denied"
        );
        assert!(
            f.is_relevant(Path::new("app.py")),
            "control: tracked non-test Python stays relevant"
        );
    }

    #[test]
    fn support_inputs_reuse_cache_helpers() {
        let tmp = tempfile::tempdir().unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &workspace_request(None, &[]));
        assert!(f.is_relevant(Path::new("pytest.ini")));
        assert!(f.is_relevant(Path::new("Cargo.toml")));
        assert!(f.is_relevant(Path::new("rust-toolchain.toml")));
        assert!(f.is_relevant(Path::new(".cargo/config.toml")));
        assert!(f.is_relevant(Path::new("foo.inc")));
        assert!(f.is_relevant(Path::new("conftest.py")));

        let exact = WatchPathFilter::build(tmp.path(), &[], None, &operands(&["src/a.py"]));
        assert!(exact.is_relevant(Path::new("src/a.py")));
        assert!(!exact.is_relevant(Path::new("src/b.py")));
        assert!(exact.is_relevant(Path::new("pytest.ini")));
    }

    #[test]
    fn absolute_file_target_matches_repo_relative_event() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("src/a.py");
        let f = WatchPathFilter::build(
            tmp.path(),
            &[],
            None,
            &operands(&[&target.to_string_lossy()]),
        );
        assert!(f.is_relevant(Path::new("src/a.py")));
        assert!(!f.is_relevant(Path::new("src/b.py")));
    }

    #[test]
    fn parent_dir_file_target_matches_canonical_event() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
        std::fs::write(tmp.path().join("tests/a.py"), "x = 1\n").unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &operands(&["src/../tests/a.py"]));
        assert!(f.is_relevant(Path::new("tests/a.py")));
        assert!(!f.is_relevant(Path::new("tests/b.py")));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_file_target_matches_canonical_event() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("real.py"), "x = 1\n").unwrap();
        symlink("real.py", tmp.path().join("link.py")).unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &operands(&["link.py"]));
        assert!(f.is_relevant(Path::new("real.py")));
        assert!(!f.is_relevant(Path::new("link.py")));
    }

    #[test]
    fn mixed_file_and_directory_targets_keep_both_scopes() {
        let tmp = tempfile::tempdir().unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &operands(&["src/a.py", "tests"]));
        assert!(f.is_relevant(Path::new("src/a.py")));
        assert!(!f.is_relevant(Path::new("src/b.py")));
        assert!(f.is_relevant(Path::new("tests/test_b.py")));
        assert!(!f.is_relevant(Path::new("other/test_c.py")));
    }

    #[test]
    fn extension_suffixed_existing_directory_keeps_directory_scope() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("suite.py")).unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &operands(&["suite.py"]));
        assert!(f.is_relevant(Path::new("suite.py/test_child.py")));
    }

    #[test]
    fn cli_ignore_uses_shared_prefix_matcher() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = WatchPathFilter::build(
            tmp.path(),
            &["fake_".into()],
            None,
            &workspace_request(None, &["fake_".into()]),
        );
        assert!(!fake.is_relevant(Path::new("tests/fake_python/test_x.py")));
        assert!(fake.is_relevant(Path::new("tests/test_app.py")));

        let slow = WatchPathFilter::build(
            tmp.path(),
            &["tests/slow".into()],
            None,
            &workspace_request(None, &["tests/slow".into()]),
        );
        assert!(!slow.is_relevant(Path::new("tests/slow/test_b.py")));
        assert!(slow.is_relevant(Path::new("tests/fast/test_a.py")));
    }

    #[test]
    fn basename_exclude_is_not_an_ignore_support_file() {
        let tmp = tempfile::tempdir().unwrap();
        let f = WatchPathFilter::build(tmp.path(), &[], None, &workspace_request(None, &[]));
        assert!(!f.is_ignore_file(Path::new("vendor/exclude")));
        assert!(!f.is_ignore_file(Path::new("exclude")));
        assert!(f.is_ignore_file(Path::new(".git/info/exclude")));
        assert!(f.is_ignore_file(Path::new(".gitignore")));
        assert!(f.is_ignore_file(Path::new("nested/.gitignore")));
    }

    #[test]
    fn watch_path_filter_build_takes_target_request() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace =
            WatchPathFilter::build(tmp.path(), &[], None, &workspace_request(None, &[]));
        let commit = commit_request();
        let git = WatchPathFilter::build(tmp.path(), &[], None, &commit);
        assert!(!workspace.is_relevant(Path::new(".git/HEAD")));
        assert!(git.is_relevant(Path::new(".git/HEAD")));
    }

    #[test]
    fn commit_request_watches_git_head_workspace_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = workspace_request(None, &[]);
        let commit = commit_request();
        let all = WatchPathFilter::build_with_config(
            tmp.path(),
            &[],
            None,
            &workspace,
            Path::new(".kissconfig"),
        );
        let git = WatchPathFilter::build_with_config(
            tmp.path(),
            &[],
            None,
            &commit,
            Path::new(".kissconfig"),
        );
        assert!(!all.is_relevant(Path::new(".git/HEAD")));
        assert!(git.is_relevant(Path::new(".git/HEAD")));
        assert!(all.is_relevant(Path::new(".git/info/exclude")));
        assert!(git.is_relevant(Path::new(".git/info/exclude")));
    }
}
