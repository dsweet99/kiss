use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::Mutex;

const PY_EXT: &[u8] = b"py";
const RS_EXT: &[u8] = b"rs";
const PY_SUFFIX: &[u8] = b".py";
const RS_SUFFIX: &[u8] = b".rs";
const PY_GLOB: &str = "*.py";
const RS_GLOB: &str = "*.rs";
const CARGO_MANIFEST: &str = "Cargo.toml";
const NESTED_CARGO_MANIFEST_GLOB: &str = "**/Cargo.toml";
const CARGO_LOCK: &str = "Cargo.lock";
const NESTED_CARGO_LOCK_GLOB: &str = "**/Cargo.lock";
const CARGO_CONFIG: &str = ".cargo/config";
const CARGO_CONFIG_TOML: &str = ".cargo/config.toml";
const RUST_TOOLCHAIN: &str = "rust-toolchain";
const RUST_TOOLCHAIN_TOML: &str = "rust-toolchain.toml";

#[derive(Clone, Copy)]
enum SourceKind {
    Rust,
    Both,
}

type InventoryKey = (String, Vec<String>);

struct InventorySession {
    active_roots: BTreeMap<String, usize>,
    entries: BTreeMap<InventoryKey, (Vec<String>, Vec<String>)>,
    fingerprints: BTreeMap<InventoryKey, super::LangFingerprints>,
}

static INVENTORY_SESSION: Mutex<InventorySession> = Mutex::new(InventorySession {
    active_roots: BTreeMap::new(),
    entries: BTreeMap::new(),
    fingerprints: BTreeMap::new(),
});

pub(crate) struct InventorySessionGuard(String);

pub(crate) fn begin_inventory_session(repo_root: &Path) -> InventorySessionGuard {
    let root = super::normalized_root(repo_root);
    if let Ok(mut session) = INVENTORY_SESSION.lock() {
        *session.active_roots.entry(root.clone()).or_default() += 1;
    }
    InventorySessionGuard(root)
}

impl Drop for InventorySessionGuard {
    fn drop(&mut self) {
        if let Ok(mut session) = INVENTORY_SESSION.lock()
            && let Some(count) = session.active_roots.get_mut(&self.0)
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                session.active_roots.remove(&self.0);
                session.entries.retain(|(root, _), _| root != &self.0);
                session.fingerprints.retain(|(root, _), _| root != &self.0);
            }
        }
    }
}

fn ignored(rel: &str, ignore: &[String]) -> bool {
    kiss::path_ignored_by_prefixes(rel, ignore)
}

fn should_skip_dir(name: &str) -> bool {
    matches!(
        name.as_bytes(),
        b".git"
            | b"target"
            | b".kiss"
            | b".venv"
            | b"venv"
            | b"__pycache__"
            | b".pytest_cache"
            | b".rslip_cache"
            | b"node_modules"
    )
}

fn is_py_ext(ext: &OsStr) -> bool {
    ext.as_encoded_bytes().eq_ignore_ascii_case(PY_EXT)
}

fn is_rs_ext(ext: &OsStr) -> bool {
    ext.as_encoded_bytes().eq_ignore_ascii_case(RS_EXT)
}

fn ends_with_py(rel: &str) -> bool {
    rel.as_bytes().ends_with(PY_SUFFIX)
}

fn ends_with_rs(rel: &str) -> bool {
    rel.as_bytes().ends_with(RS_SUFFIX)
}

fn is_cargo_manifest(rel: &str) -> bool {
    rel == CARGO_MANIFEST || rel.ends_with("/Cargo.toml")
}

fn is_cargo_discovery_input(rel: &str) -> bool {
    is_cargo_manifest(rel)
        || rel == CARGO_LOCK
        || rel.ends_with("/Cargo.lock")
        || rel == CARGO_CONFIG
        || rel == CARGO_CONFIG_TOML
        || rel.ends_with("/.cargo/config")
        || rel.ends_with("/.cargo/config.toml")
        || rel == RUST_TOOLCHAIN
        || rel == RUST_TOOLCHAIN_TOML
        || rel.ends_with("/rust-toolchain")
        || rel.ends_with("/rust-toolchain.toml")
}

fn list_sources_git(
    repo_root: &Path,
    ignore: &[String],
    kind: SourceKind,
) -> io::Result<(Vec<String>, Vec<String>)> {
    let mut cmd = kiss::scrubbed_git_command(repo_root);
    cmd.args(["ls-files", "-z", "-c", "-o", "--exclude-standard", "--"]);
    if matches!(kind, SourceKind::Both) {
        cmd.arg(PY_GLOB);
    }
    if matches!(kind, SourceKind::Rust | SourceKind::Both) {
        cmd.arg(RS_GLOB);
        cmd.arg(CARGO_MANIFEST);
        cmd.arg(NESTED_CARGO_MANIFEST_GLOB);
        cmd.arg(CARGO_LOCK);
        cmd.arg(NESTED_CARGO_LOCK_GLOB);
        cmd.args([
            CARGO_CONFIG,
            CARGO_CONFIG_TOML,
            "**/.cargo/config",
            "**/.cargo/config.toml",
            RUST_TOOLCHAIN,
            RUST_TOOLCHAIN_TOML,
            "**/rust-toolchain",
            "**/rust-toolchain.toml",
        ]);
    }
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(io::Error::other("git ls-files failed"));
    }
    let mut py_rels = Vec::new();
    let mut rs_rels = Vec::new();
    for part in output
        .stdout
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
    {
        let rel = String::from_utf8_lossy(part).replace('\\', "/");
        if ignored(&rel, ignore) {
            continue;
        }
        if ends_with_py(&rel) {
            py_rels.push(rel);
        } else if ends_with_rs(&rel) || is_cargo_discovery_input(&rel) {
            rs_rels.push(rel);
        }
    }
    py_rels.sort();
    py_rels.dedup();
    rs_rels.sort();
    rs_rels.dedup();
    Ok((py_rels, rs_rels))
}

fn list_sources_walk(
    repo_root: &Path,
    ignore: &[String],
    kind: SourceKind,
) -> io::Result<(Vec<String>, Vec<String>)> {
    let mut py_rels = Vec::new();
    let mut rs_rels = Vec::new();
    let mut stack = vec![repo_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)?;
        let mut paths: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            let rel = path
                .strip_prefix(repo_root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if path.is_dir() {
                if should_skip_dir(name) || ignored(&rel, ignore) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if ignored(&rel, ignore) {
                continue;
            }
            if matches!(kind, SourceKind::Both) && path.extension().is_some_and(is_py_ext) {
                py_rels.push(rel);
            } else if matches!(kind, SourceKind::Rust | SourceKind::Both)
                && (path.extension().is_some_and(is_rs_ext) || is_cargo_discovery_input(&rel))
            {
                rs_rels.push(rel);
            }
        }
    }
    py_rels.sort();
    py_rels.dedup();
    rs_rels.sort();
    rs_rels.dedup();
    Ok((py_rels, rs_rels))
}

fn list_source_rels(
    repo_root: &Path,
    ignore: &[String],
    kind: SourceKind,
) -> Option<(Vec<String>, Vec<String>)> {
    let key = (super::normalized_root(repo_root), ignore.to_vec());
    if let Ok(session) = INVENTORY_SESSION.lock()
        && session.active_roots.contains_key(&key.0)
        && let Some((python, rust)) = session.entries.get(&key)
    {
        return Some(match kind {
            SourceKind::Rust => (Vec::new(), rust.clone()),
            SourceKind::Both => (python.clone(), rust.clone()),
        });
    }
    let listed = list_sources_git(repo_root, ignore, SourceKind::Both)
        .or_else(|_| list_sources_walk(repo_root, ignore, SourceKind::Both))
        .ok()?;
    if let Ok(mut session) = INVENTORY_SESSION.lock()
        && session.active_roots.contains_key(&key.0)
    {
        session.entries.insert(key, listed.clone());
    }
    Some(match kind {
        SourceKind::Rust => (Vec::new(), listed.1),
        SourceKind::Both => listed,
    })
}

pub(super) fn rust_source_rels(repo_root: &Path, ignore: &[String]) -> io::Result<Vec<String>> {
    list_source_rels(repo_root, ignore, SourceKind::Rust)
        .map(|(_, rust)| rust)
        .ok_or_else(|| io::Error::other("list Rust sources failed"))
}

pub(super) fn remember_source_rels(
    repo_root: &Path,
    ignore: &[String],
    python: &[String],
    rust: &[String],
) {
    let key = (super::normalized_root(repo_root), ignore.to_vec());
    if let Ok(mut session) = INVENTORY_SESSION.lock()
        && session.active_roots.contains_key(&key.0)
    {
        session
            .entries
            .insert(key, (python.to_vec(), rust.to_vec()));
    }
}

pub(super) fn recall_fingerprints(
    repo_root: &Path,
    ignore: &[String],
) -> Option<super::LangFingerprints> {
    let key = (super::normalized_root(repo_root), ignore.to_vec());
    let session = INVENTORY_SESSION.lock().ok()?;
    if !session.active_roots.contains_key(&key.0) {
        return None;
    }
    session.fingerprints.get(&key).cloned()
}

pub(super) fn remember_fingerprints(
    repo_root: &Path,
    ignore: &[String],
    fingerprints: &super::LangFingerprints,
) {
    let key = (super::normalized_root(repo_root), ignore.to_vec());
    if let Ok(mut session) = INVENTORY_SESSION.lock()
        && session.active_roots.contains_key(&key.0)
    {
        session.fingerprints.insert(key, fingerprints.clone());
    }
}
