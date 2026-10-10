use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TOOL_VERSIONS_SCHEMA: &str = "rust-toolchain-versions-v1";

#[path = "toolchain_host_cache.rs"]
mod host_cache;
#[path = "toolchain_live.rs"]
mod live;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RustToolchain {
    pub(crate) cargo: String,
    pub(crate) rustc: String,
    pub(crate) cargo_nextest: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct RustToolVersionsCache {
    schema_version: String,
    tools: RustToolchain,
    key: PersistedToolIdentityCacheKey,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
struct PersistedFileMeta {
    path: PathBuf,
    len: u64,
    mtime_nanos: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct PersistedToolIdentityCacheKey {
    cargo: PathBuf,
    rustc: PathBuf,
    cargo_meta: Option<(u64, Option<u64>)>,
    cargo_nextest_meta: Option<(u64, Option<u64>)>,
    config_metas: Vec<PersistedFileMeta>,
    toolchain_metas: Vec<PersistedFileMeta>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ToolIdentityCacheKey {
    cargo: PathBuf,
    rustc: PathBuf,
    cargo_meta: Option<(u64, Option<SystemTime>)>,
    cargo_nextest_meta: Option<(u64, Option<SystemTime>)>,
    config_metas: Vec<(PathBuf, u64, Option<SystemTime>)>,
    toolchain_metas: Vec<(PathBuf, u64, Option<SystemTime>)>,
}

struct ToolIdentityCache {
    key: ToolIdentityCacheKey,
    tools: RustToolchain,
}

static TOOLS_CACHE: Mutex<Option<ToolIdentityCache>> = Mutex::new(None);

fn rust_tool_versions_cache_path(repo_root: &Path) -> PathBuf {
    crate::test_runner::test_state_dir(repo_root).join("rust_toolchain_versions.json")
}

pub(super) fn system_time_to_nanos(ts: SystemTime) -> Option<u64> {
    ts.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

fn nanos_to_system_time(nanos: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_nanos(nanos)
}

fn persist_meta(meta: Option<(u64, Option<SystemTime>)>) -> Option<(u64, Option<u64>)> {
    meta.map(|(len, mtime)| (len, mtime.and_then(system_time_to_nanos)))
}

fn restore_meta(meta: Option<(u64, Option<u64>)>) -> Option<(u64, Option<SystemTime>)> {
    meta.map(|(len, mtime)| (len, mtime.map(nanos_to_system_time)))
}

fn persist_path_metas(metas: &[(PathBuf, u64, Option<SystemTime>)]) -> Vec<PersistedFileMeta> {
    metas
        .iter()
        .map(|(path, len, mtime)| PersistedFileMeta {
            path: path.clone(),
            len: *len,
            mtime_nanos: mtime.and_then(system_time_to_nanos),
        })
        .collect()
}

fn restore_path_metas(metas: &[PersistedFileMeta]) -> Vec<(PathBuf, u64, Option<SystemTime>)> {
    metas
        .iter()
        .map(|m| {
            (
                m.path.clone(),
                m.len,
                m.mtime_nanos.map(nanos_to_system_time),
            )
        })
        .collect()
}

impl ToolIdentityCacheKey {
    pub(super) fn to_persisted(&self) -> PersistedToolIdentityCacheKey {
        PersistedToolIdentityCacheKey {
            cargo: self.cargo.clone(),
            rustc: self.rustc.clone(),
            cargo_meta: persist_meta(self.cargo_meta),
            cargo_nextest_meta: persist_meta(self.cargo_nextest_meta),
            config_metas: persist_path_metas(&self.config_metas),
            toolchain_metas: persist_path_metas(&self.toolchain_metas),
        }
    }

    fn from_persisted(key: PersistedToolIdentityCacheKey) -> Self {
        Self {
            cargo: key.cargo,
            rustc: key.rustc,
            cargo_meta: restore_meta(key.cargo_meta),
            cargo_nextest_meta: restore_meta(key.cargo_nextest_meta),
            config_metas: restore_path_metas(&key.config_metas),
            toolchain_metas: restore_path_metas(&key.toolchain_metas),
        }
    }
}

fn read_cached_rust_tool_identity(
    repo_root: &Path,
) -> Option<(ToolIdentityCacheKey, RustToolchain)> {
    let bytes = fs::read(rust_tool_versions_cache_path(repo_root)).ok()?;
    let cached: RustToolVersionsCache = serde_json::from_slice(&bytes).ok()?;
    if cached.schema_version != TOOL_VERSIONS_SCHEMA {
        return None;
    }
    Some((
        ToolIdentityCacheKey::from_persisted(cached.key),
        cached.tools,
    ))
}

fn write_cached_rust_tool_identity(
    repo_root: &Path,
    key: &ToolIdentityCacheKey,
    tools: &RustToolchain,
) -> std::io::Result<()> {
    let path = rust_tool_versions_cache_path(repo_root);
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "rust_toolchain_versions path has no parent",
        )
    })?;
    let tmp = parent.join(format!(
        ".rust_toolchain_versions.{}.tmp",
        kiss::kiss_publication_barrier::unique_process_suffix()
    ));
    let cached = RustToolVersionsCache {
        schema_version: TOOL_VERSIONS_SCHEMA.to_string(),
        tools: tools.clone(),
        key: key.to_persisted(),
    };
    kiss::kiss_publication_barrier::publish_atomically(
        "rust_toolchain_versions",
        &path,
        &tmp,
        |file| {
            serde_json::to_writer(&mut *file, &cached).map_err(std::io::Error::other)?;
            file.write_all(b"\n")?;
            Ok(())
        },
    )
}

fn file_meta(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

fn collect_config_metas(repo_root: &Path) -> Vec<(PathBuf, u64, Option<SystemTime>)> {
    let mut out = Vec::new();
    for name in ["config", "config.toml"] {
        let path = repo_root.join(".cargo").join(name);
        if let Some((len, mtime)) = file_meta(&path) {
            out.push((path, len, mtime));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn collect_toolchain_metas(repo_root: &Path) -> Vec<(PathBuf, u64, Option<SystemTime>)> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(repo_root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with("rust-toolchain") {
                let path = entry.path();
                if let Some((len, mtime)) = file_meta(&path) {
                    out.push((path, len, mtime));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn build_tool_identity_cache_key(repo_root: &Path) -> ToolIdentityCacheKey {
    let cargo = PathBuf::from("cargo");
    let rustc = PathBuf::from("rustc");
    ToolIdentityCacheKey {
        cargo_meta: which_meta(&cargo),
        cargo_nextest_meta: which_meta(Path::new("cargo-nextest")),
        config_metas: collect_config_metas(repo_root),
        toolchain_metas: collect_toolchain_metas(repo_root),
        cargo,
        rustc,
    }
}

fn which_meta(program: &Path) -> Option<(u64, Option<SystemTime>)> {
    file_meta(program).or_else(|| {
        std::env::var_os("PATH").and_then(|paths| {
            for dir in std::env::split_paths(&paths) {
                let candidate = dir.join(program);
                if let Some(meta) = file_meta(&candidate) {
                    return Some(meta);
                }
            }
            None
        })
    })
}

fn detect_rust_toolchain(
    repo_root: &Path,
    key: &ToolIdentityCacheKey,
) -> Result<RustToolchain, String> {
    if let Some((cached_key, tools)) = read_cached_rust_tool_identity(repo_root)
        && cached_key == *key
    {
        return Ok(tools);
    }
    if let Some(tools) = host_cache::read_host_cached_rust_toolchain(key) {
        let _ = write_cached_rust_tool_identity(repo_root, key, &tools);
        return Ok(tools);
    }
    let live = live::detect_live_rust_toolchain(repo_root)?;
    let _ = write_cached_rust_tool_identity(repo_root, key, &live);
    let _ = host_cache::write_host_cached_rust_toolchain(key, &live);
    Ok(live)
}

pub(crate) fn current_rust_toolchain(repo_root: &Path) -> Result<RustToolchain, String> {
    let key = build_tool_identity_cache_key(repo_root);
    let mut guard = TOOLS_CACHE.lock().expect("tool identity cache lock");
    if let Some(cached) = guard.as_ref()
        && cached.key == key
    {
        return Ok(cached.tools.clone());
    }
    let tools = detect_rust_toolchain(repo_root, &key)?;
    *guard = Some(ToolIdentityCache {
        key,
        tools: tools.clone(),
    });
    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyed_cache_changes_when_toolchain_file_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let key1 = build_tool_identity_cache_key(tmp.path());
        std::fs::write(
            tmp.path().join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.0\"\n",
        )
        .unwrap();
        let key2 = build_tool_identity_cache_key(tmp.path());
        assert_ne!(key1, key2);
    }

    #[test]
    fn disk_cache_hit_skips_live_probe_when_key_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let key = build_tool_identity_cache_key(tmp.path());
        let tools = RustToolchain {
            cargo: "cargo-test".into(),
            rustc: "rustc-test".into(),
            cargo_nextest: "nextest-test".into(),
        };
        write_cached_rust_tool_identity(tmp.path(), &key, &tools).unwrap();
        let (loaded_key, loaded) = read_cached_rust_tool_identity(tmp.path()).expect("cache hit");
        assert_eq!(loaded_key, key);
        assert_eq!(loaded, tools);
        let again = detect_rust_toolchain(tmp.path(), &key).expect("trusted disk");
        assert_eq!(again, tools);
    }

    #[test]
    fn disk_cache_miss_when_key_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let key = build_tool_identity_cache_key(tmp.path());
        let tools = RustToolchain {
            cargo: "cargo-test".into(),
            rustc: "rustc-test".into(),
            cargo_nextest: "nextest-test".into(),
        };
        write_cached_rust_tool_identity(tmp.path(), &key, &tools).unwrap();
        std::fs::write(
            tmp.path().join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"nightly\"\n",
        )
        .unwrap();
        let changed = build_tool_identity_cache_key(tmp.path());
        assert_ne!(key, changed);
        let (loaded_key, _) = read_cached_rust_tool_identity(tmp.path()).expect("readable");
        assert_ne!(loaded_key, changed);
    }

    #[test]
    fn cache_without_schema_is_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let path = rust_tool_versions_cache_path(tmp.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"tools":{"cargo":"c","rustc":"r","cargo_nextest":"n"}}"#,
        )
        .unwrap();
        assert!(read_cached_rust_tool_identity(tmp.path()).is_none());
    }
}
