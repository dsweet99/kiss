use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use kiss::rust_llvm_cov_runner::RustCoverageToolIdentity;

use super::{PersistedToolIdentityCacheKey, ToolIdentityCacheKey};

const HOST_TOOL_VERSIONS_SCHEMA: &str = "rust-host-tool-versions-v1";
const HOST_TOOL_VERSIONS_CAP: usize = 32;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct HostToolVersionsCache {
    schema_version: String,
    entries: Vec<HostToolVersionsEntry>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct HostToolVersionsEntry {
    key: PersistedToolIdentityCacheKey,
    cargo: String,
    llvm_cov: String,
    rustc: String,
    cargo_nextest: String,
}

fn host_tool_versions_cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("kiss").join("rust_tool_versions.json"))
}

pub(super) fn read_host_cached_rust_tool_identity(
    key: &ToolIdentityCacheKey,
) -> Option<RustCoverageToolIdentity> {
    let path = host_tool_versions_cache_path()?;
    let bytes = fs::read(path).ok()?;
    let cached: HostToolVersionsCache = serde_json::from_slice(&bytes).ok()?;
    if cached.schema_version != HOST_TOOL_VERSIONS_SCHEMA {
        return None;
    }
    let wanted = key.to_persisted();
    cached.entries.into_iter().rev().find_map(|entry| {
        (entry.key == wanted).then_some(RustCoverageToolIdentity {
            cargo_version: entry.cargo,
            llvm_cov_version: entry.llvm_cov,
            rustc_version: entry.rustc,
            cargo_nextest_version: entry.cargo_nextest,
        })
    })
}

pub(super) fn write_host_cached_rust_tool_identity(
    key: &ToolIdentityCacheKey,
    tools: &RustCoverageToolIdentity,
) -> std::io::Result<()> {
    let path = host_tool_versions_cache_path().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no XDG_CACHE_HOME or HOME for host tool identity cache",
        )
    })?;
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "host rust_tool_versions path has no parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    let mut cached = read_host_cache_file(&path).unwrap_or_else(|| HostToolVersionsCache {
        schema_version: HOST_TOOL_VERSIONS_SCHEMA.to_string(),
        entries: Vec::new(),
    });
    if cached.schema_version != HOST_TOOL_VERSIONS_SCHEMA {
        cached = HostToolVersionsCache {
            schema_version: HOST_TOOL_VERSIONS_SCHEMA.to_string(),
            entries: Vec::new(),
        };
    }
    let persisted = key.to_persisted();
    cached.entries.retain(|entry| entry.key != persisted);
    cached.entries.push(HostToolVersionsEntry {
        key: persisted,
        cargo: tools.cargo_version.clone(),
        llvm_cov: tools.llvm_cov_version.clone(),
        rustc: tools.rustc_version.clone(),
        cargo_nextest: tools.cargo_nextest_version.clone(),
    });
    if cached.entries.len() > HOST_TOOL_VERSIONS_CAP {
        let drop = cached.entries.len() - HOST_TOOL_VERSIONS_CAP;
        cached.entries.drain(0..drop);
    }
    let tmp = parent.join(format!(
        ".rust_tool_versions.{}.tmp",
        kiss::kiss_publication_barrier::unique_process_suffix()
    ));
    kiss::kiss_publication_barrier::publish_atomically(
        "host_rust_tool_versions",
        &path,
        &tmp,
        |file| {
            serde_json::to_writer(&mut *file, &cached).map_err(std::io::Error::other)?;
            file.write_all(b"\n")?;
            Ok(())
        },
    )
}

fn read_host_cache_file(path: &Path) -> Option<HostToolVersionsCache> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_cache_round_trip_under_xdg() {
        let tmp = tempfile::tempdir().unwrap();
        let xdg = tmp.path().join("xdg");
        fs::create_dir_all(&xdg).unwrap();
        let _guard =
            crate::test_runner::TestEnvVarGuard::set("XDG_CACHE_HOME", xdg.to_str().unwrap());
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let key = super::super::build_tool_identity_cache_key(&repo);
        let tools = RustCoverageToolIdentity {
            cargo_version: "cargo-host".into(),
            llvm_cov_version: "llvm-host".into(),
            rustc_version: "rustc-host".into(),
            cargo_nextest_version: "nextest-host".into(),
        };
        write_host_cached_rust_tool_identity(&key, &tools).unwrap();
        let loaded = read_host_cached_rust_tool_identity(&key).expect("host hit");
        assert_eq!(loaded, tools);
    }
}
