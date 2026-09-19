use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ignore::gitignore::Gitignore;

use crate::analyze_cache::fnv1a64;
use crate::test_runner::workspace_selector_cache::{
    should_skip_dir, watch_support_gitignore,
};

const SUPPORT_SEED: &[u8] = b"suite-report-watch-support-v3";

pub(super) struct SuiteDigests {
    pub all: String,
    pub python: String,
    pub rust: String,
}

pub(super) fn suite_source_digests(repo: &Path, ignore: &[String]) -> io::Result<SuiteDigests> {
    let (python, _) =
        crate::test_runner::workspace_selector_cache::workspace_lang_file_fingerprints(
            repo, ignore,
        )?;
    let rust = crate::test_runner::workspace_selector_cache::rust_full_source_fingerprint(
        repo, ignore,
    )?;
    let support = watch_support_fingerprint(repo, ignore)?;
    Ok(SuiteDigests {
        all: format!("{python}:{rust}:{support}"),
        python: format!("{python}:{support}"),
        rust: format!("{rust}:{support}"),
    })
}

fn watch_support_fingerprint(repo: &Path, ignore: &[String]) -> io::Result<String> {
    let mut rels = support_rels_walk(repo, ignore)?;
    if repo.join(".git/info/exclude").is_file() {
        rels.push(".git/info/exclude".into());
    }
    rels.sort();
    rels.dedup();
    let mut h = hash_named(repo, &rels)?;
    h = mix_watched_config(repo, h)?;
    Ok(format!("{h:016x}"))
}

fn support_rels_walk(repo: &Path, ignore: &[String]) -> io::Result<Vec<String>> {
    let gitignore = watch_support_gitignore(repo);
    let mut rels = Vec::new();
    let mut stack = vec![repo.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            collect_support_entry(
                repo,
                ignore,
                &gitignore,
                entry?.path(),
                &mut stack,
                &mut rels,
            );
        }
    }
    Ok(rels)
}

fn collect_support_entry(
    repo: &Path,
    ignore: &[String],
    gitignore: &Gitignore,
    path: PathBuf,
    stack: &mut Vec<PathBuf>,
    rels: &mut Vec<String>,
) {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let rel = path
        .strip_prefix(repo)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    if path.is_dir() {
        if !should_skip_dir(name) && !skipped_by_watch(repo, ignore, gitignore, &rel, true) {
            stack.push(path);
        }
        return;
    }
    if !skipped_by_watch(repo, ignore, gitignore, &rel, false) && is_support_rel(&rel) {
        rels.push(rel);
    }
}

fn hash_named(repo: &Path, rels: &[String]) -> io::Result<u64> {
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, SUPPORT_SEED);
    for rel in rels {
        h = fnv1a64(h, rel.as_bytes());
        h = fnv1a64(h, &[0]);
        h = fnv1a64(h, &fs::read(repo.join(rel))?);
        h = fnv1a64(h, &[0]);
    }
    Ok(h)
}

fn mix_watched_config(repo: &Path, mut h: u64) -> io::Result<u64> {
    let path = resolve_active_config(repo);
    let rel = path
        .strip_prefix(repo)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
    h = fnv1a64(h, b"watched-config-v1");
    h = fnv1a64(h, rel.as_bytes());
    if path.is_file() {
        h = fnv1a64(h, &fs::read(&path)?);
    } else {
        h = fnv1a64(h, b"absent");
    }
    Ok(h)
}

fn resolve_active_config(repo: &Path) -> PathBuf {
    let active = kiss::active_kissconfig_path();
    if active == kiss::kissconfig_path_from_cwd() {
        return repo.join(".kissconfig");
    }
    if active.is_absolute() {
        active
    } else {
        repo.join(active)
    }
}

fn is_support_rel(rel: &str) -> bool {
    let path = Path::new(rel);
    let name = path.file_name().and_then(|name| name.to_str());
    matches!(
        name,
        Some(
            ".gitignore"
                | ".kissignore"
                | "pytest.ini"
                | "pyproject.toml"
                | "setup.cfg"
                | "tox.ini"
                | "config.toml"
        )
    ) || path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("inc"))
        || name.is_some_and(|name| name.starts_with("rust-toolchain"))
}

fn skipped_by_watch(
    repo: &Path,
    ignore: &[String],
    gitignore: &Gitignore,
    rel: &str,
    is_dir: bool,
) -> bool {
    kiss::path_ignored_by_prefixes(rel, ignore)
        || gitignore.matched(repo.join(rel), is_dir).is_ignore()
}
