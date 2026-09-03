use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::analyze_cache::fnv1a64;

const DISK_SCHEMA: &str = "selector-source-digests-v4";
const DISK_FILE: &str = "selector_source_digests.json";

#[derive(Clone, Eq, PartialEq, Hash)]
struct FileStamp {
    path: PathBuf,
    len: u64,
    mtime_ns: u64,
    ctime_ns: u64,
    dev: u64,
    ino: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct DiskRecord {
    len: u64,
    mtime_ns: u64,
    ctime_ns: u64,
    dev: u64,
    ino: u64,
    digest: u64,
    has_literal_includes: bool,
}

#[derive(Serialize, Deserialize)]
struct DiskFile {
    schema_version: String,
    files: BTreeMap<String, DiskRecord>,
}

fn memo() -> &'static Mutex<HashMap<FileStamp, u64>> {
    static MEMO: OnceLock<Mutex<HashMap<FileStamp, u64>>> = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

fn disk_maps() -> &'static Mutex<HashMap<PathBuf, BTreeMap<String, DiskRecord>>> {
    static MAPS: OnceLock<Mutex<HashMap<PathBuf, BTreeMap<String, DiskRecord>>>> = OnceLock::new();
    MAPS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn system_time_ns(ts: SystemTime) -> u64 {
    ts.duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn ctime_ns(meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let sec = u64::try_from(meta.ctime()).unwrap_or(0);
        let nsec = u64::try_from(meta.ctime_nsec()).unwrap_or(0);
        sec.saturating_mul(1_000_000_000).saturating_add(nsec)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0
    }
}

fn device_inode(meta: &fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        (0, 0)
    }
}

fn stamp_for(path: &Path, meta: &fs::Metadata) -> FileStamp {
    let (dev, ino) = device_inode(meta);
    FileStamp {
        path: path.to_path_buf(),
        len: meta.len(),
        mtime_ns: meta.modified().map(system_time_ns).unwrap_or(0),
        ctime_ns: ctime_ns(meta),
        dev,
        ino,
    }
}

fn record_matches(record: &DiskRecord, stamp: &FileStamp) -> bool {
    record.len == stamp.len
        && record.mtime_ns == stamp.mtime_ns
        && record.ctime_ns == stamp.ctime_ns
        && record.dev == stamp.dev
        && record.ino == stamp.ino
}

fn load_disk_map(repo_root: &Path) -> BTreeMap<String, DiskRecord> {
    let path = repo_root.join(".kiss").join(DISK_FILE);
    let Ok(bytes) = fs::read(path) else {
        return BTreeMap::new();
    };
    let Ok(parsed) = serde_json::from_slice::<DiskFile>(&bytes) else {
        return BTreeMap::new();
    };
    if parsed.schema_version != DISK_SCHEMA {
        return BTreeMap::new();
    }
    parsed.files
}

fn with_repo_disk_map<R>(
    repo_root: &Path,
    f: impl FnOnce(&BTreeMap<String, DiskRecord>) -> R,
) -> R {
    let key = repo_root.to_path_buf();
    let Ok(mut guard) = disk_maps().lock() else {
        return f(&BTreeMap::new());
    };
    if !guard.contains_key(&key) {
        let loaded = load_disk_map(repo_root);
        guard.insert(key.clone(), loaded);
    }
    match guard.get(&key) {
        Some(map) => f(map),
        None => f(&BTreeMap::new()),
    }
}

fn remember_disk_record(
    repo_root: &Path,
    rel: &str,
    stamp: &FileStamp,
    digest: u64,
    has_literal_includes: bool,
) {
    let key = repo_root.to_path_buf();
    let record = DiskRecord {
        len: stamp.len,
        mtime_ns: stamp.mtime_ns,
        ctime_ns: stamp.ctime_ns,
        dev: stamp.dev,
        ino: stamp.ino,
        digest,
        has_literal_includes,
    };
    if let Ok(mut guard) = disk_maps().lock() {
        guard
            .entry(key)
            .or_default()
            .insert(rel.to_string(), record);
    }
}

fn content_digest(repo_root: &Path, rel: &str, path: &Path, stamp: &FileStamp) -> io::Result<u64> {
    let cached = with_repo_disk_map(repo_root, |disk| {
        disk.get(rel)
            .filter(|record| record_matches(record, stamp))
            .cloned()
    });
    if let Some(record) = cached.as_ref()
        && !record.has_literal_includes
    {
        if let Ok(mut guard) = memo().lock() {
            guard.insert(stamp.clone(), record.digest);
        }
        return Ok(record.digest);
    }
    if cached.is_none()
        && let Ok(guard) = memo().lock()
        && let Some(digest) = guard.get(stamp).copied()
    {
        return Ok(digest);
    }
    let bytes = fs::read(path)?;
    let mut hashed = if rel.ends_with(".rs") && !rel.ends_with("build.rs") {
        rust_selector_declaration_bytes(&bytes)
    } else {
        bytes.clone()
    };
    let has_literal_includes = rel.ends_with(".rs")
        && append_literal_include_dependencies(
            repo_root,
            path,
            &String::from_utf8_lossy(&bytes),
            &mut hashed,
        );
    let digest = fnv1a64(0xcbf2_9ce4_8422_2325, &hashed);
    if !has_literal_includes && let Ok(mut guard) = memo().lock() {
        guard.insert(stamp.clone(), digest);
    }
    remember_disk_record(repo_root, rel, stamp, digest, has_literal_includes);
    Ok(digest)
}

fn append_literal_include_dependencies(
    repo_root: &Path,
    includer: &Path,
    source: &str,
    out: &mut Vec<u8>,
) -> bool {
    let first = include_literals(source, &cargo_manifest_dir(includer, repo_root));
    if first.is_empty() {
        return false;
    }
    let canonical_root = kiss::rust_include::canonical_path(repo_root);
    let mut queue: VecDeque<_> = first
        .into_iter()
        .map(|literal| (includer.to_path_buf(), literal))
        .collect();
    let mut seen = BTreeSet::new();
    let mut dependencies = BTreeMap::new();
    while let Some((parent, literal)) = queue.pop_front() {
        let resolved = kiss::rust_include::resolve_include_path(&parent, &literal);
        let canonical = kiss::rust_include::canonical_path(&resolved);
        let Ok(relative) = canonical.strip_prefix(&canonical_root) else {
            continue;
        };
        let rel = relative.to_string_lossy().replace('\\', "/");
        if !seen.insert(rel.clone()) {
            continue;
        }
        match fs::read(&canonical) {
            Ok(bytes) => {
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    queue.extend(
                        include_literals(text, &cargo_manifest_dir(&canonical, repo_root))
                            .into_iter()
                            .map(|nested| (canonical.clone(), nested)),
                    );
                }
                dependencies.insert(rel, bytes);
            }
            Err(_) => {
                dependencies.insert(rel, b"<missing>".to_vec());
            }
        }
        if seen.len() >= 10_000 {
            break;
        }
    }
    for (rel, bytes) in dependencies {
        out.extend_from_slice(b"\ninclude-dependency:");
        out.extend_from_slice(rel.as_bytes());
        out.push(0);
        out.extend_from_slice(&bytes);
        out.push(0);
    }
    true
}

fn cargo_manifest_dir(includer: &Path, repo_root: &Path) -> PathBuf {
    let mut current = includer.parent();
    while let Some(dir) = current {
        if dir.join("Cargo.toml").is_file() {
            return dir.to_path_buf();
        }
        if dir == repo_root {
            break;
        }
        current = dir.parent();
    }
    repo_root.to_path_buf()
}

fn include_literals(source: &str, cargo_manifest_dir: &Path) -> Vec<String> {
    struct Collector<'a> {
        cargo_manifest_dir: &'a Path,
        literals: Vec<String>,
    }
    impl<'ast> syn::visit::Visit<'ast> for Collector<'_> {
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if let Some(literal) = resolved_include_expression(mac, self.cargo_manifest_dir) {
                self.literals.push(literal);
            }
            syn::visit::visit_macro(self, mac);
        }
    }
    use syn::visit::Visit;
    let Ok(file) = syn::parse_file(source) else {
        return Vec::new();
    };
    let mut collector = Collector {
        cargo_manifest_dir,
        literals: Vec::new(),
    };
    collector.visit_file(&file);
    collector.literals
}

fn resolved_include_expression(mac: &syn::Macro, cargo_manifest_dir: &Path) -> Option<String> {
    if let Some(literal) = kiss::rust_include::extract_include_literal_from_macro(mac) {
        return Some(literal);
    }
    if !mac.path.is_ident("include") {
        return None;
    }
    let expression: syn::Expr = syn::parse2(mac.tokens.clone()).ok()?;
    resolve_include_expression_part(&expression, cargo_manifest_dir)
}

fn resolve_include_expression_part(
    expression: &syn::Expr,
    cargo_manifest_dir: &Path,
) -> Option<String> {
    match expression {
        syn::Expr::Lit(literal) => match &literal.lit {
            syn::Lit::Str(value) => Some(value.value()),
            _ => None,
        },
        syn::Expr::Macro(expression_macro) if expression_macro.mac.path.is_ident("env") => {
            let variable: syn::LitStr = syn::parse2(expression_macro.mac.tokens.clone()).ok()?;
            (variable.value() == "CARGO_MANIFEST_DIR")
                .then(|| cargo_manifest_dir.to_string_lossy().to_string())
        }
        syn::Expr::Macro(expression_macro) if expression_macro.mac.path.is_ident("concat") => {
            use syn::parse::Parser;
            let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
            let parts = parser.parse2(expression_macro.mac.tokens.clone()).ok()?;
            let mut resolved = String::new();
            for part in parts {
                resolved.push_str(&resolve_include_expression_part(&part, cargo_manifest_dir)?);
            }
            Some(resolved)
        }
        _ => None,
    }
}

pub(super) fn hash_file_contents(
    h: u64,
    rel: &str,
    repo_root: &Path,
    path: &Path,
) -> io::Result<u64> {
    let meta = fs::metadata(path)?;
    let stamp = stamp_for(path, &meta);
    let digest = content_digest(repo_root, rel, path, &stamp)?;
    let acc = fnv1a64(h, rel.as_bytes());
    Ok(fnv1a64(acc, &digest.to_le_bytes()))
}

pub(super) fn flush_persisted_digests(repo_root: &Path) {
    let key = repo_root.to_path_buf();
    let Some(files) = disk_maps()
        .lock()
        .ok()
        .and_then(|guard| guard.get(&key).cloned())
    else {
        return;
    };
    let dir = repo_root.join(".kiss");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let body = DiskFile {
        schema_version: DISK_SCHEMA.to_string(),
        files,
    };
    let Ok(bytes) = serde_json::to_vec(&body) else {
        return;
    };
    let _ = fs::write(dir.join(DISK_FILE), bytes);
}

#[path = "workspace_selector_cache_digest_sig.rs"]
mod digest_sig;
use digest_sig::rust_selector_declaration_bytes;
