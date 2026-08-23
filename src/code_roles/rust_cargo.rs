use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;

use super::error::RoleBuildError;
use super::types::CodeContextSet;

#[derive(Clone, Debug)]
pub struct CargoRoot {
    pub src_path: PathBuf,
    pub allow_production: bool,
    pub workspace: PathBuf,
    pub package: String,
    pub kinds: Vec<String>,
    pub manifest_path: PathBuf,
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<MetaPackage>,
    workspace_root: PathBuf,
}

#[derive(Deserialize)]
struct MetaPackage {
    name: String,
    manifest_path: PathBuf,
    targets: Vec<MetaTarget>,
}

#[derive(Deserialize)]
struct MetaTarget {
    kind: Vec<String>,
    src_path: PathBuf,
}

pub fn cargo_roots_for_files(
    files: &[PathBuf],
) -> Result<(Vec<CargoRoot>, HashMap<PathBuf, PathBuf>), RoleBuildError> {
    let mut workspace_memo: HashMap<PathBuf, PathBuf> = HashMap::new();
    let mut metadata_memo: HashMap<PathBuf, Vec<CargoRoot>> = HashMap::new();
    let mut roots = Vec::new();
    let mut file_workspace = HashMap::new();
    for file in files {
        let Some(manifest) = nearest_manifest(file) else {
            continue;
        };
        let workspace = locate_workspace(&manifest, &mut workspace_memo)?;
        file_workspace.insert(file.clone(), workspace.clone());
        if let Some(existing) = metadata_memo.get(&workspace)
            && roots.iter().all(|r: &CargoRoot| {
                existing
                    .iter()
                    .any(|e| e.src_path == r.src_path && e.allow_production == r.allow_production)
            })
        {
            continue;
        }
        if !metadata_memo.contains_key(&workspace) {
            let workspace_roots = load_workspace_roots(&workspace)?;
            metadata_memo.insert(workspace.clone(), workspace_roots);
        }
    }
    for ws_roots in metadata_memo.into_values() {
        roots.extend(ws_roots);
    }
    Ok((roots, file_workspace))
}

fn nearest_manifest(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        let candidate = ancestor.join("Cargo.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn locate_workspace(
    manifest: &Path,
    memo: &mut HashMap<PathBuf, PathBuf>,
) -> Result<PathBuf, RoleBuildError> {
    if let Some(hit) = memo.get(manifest) {
        return Ok(hit.clone());
    }
    let root = workspace_manifest_for(manifest);
    memo.insert(manifest.to_path_buf(), root.clone());
    Ok(root)
}

fn workspace_manifest_for(package_manifest: &Path) -> PathBuf {
    if manifest_has_workspace_table(package_manifest) {
        return package_manifest.to_path_buf();
    }
    let start = package_manifest.parent().unwrap_or(package_manifest);
    for ancestor in start.ancestors() {
        let candidate = ancestor.join("Cargo.toml");
        if manifest_has_workspace_table(&candidate) {
            return candidate;
        }
    }
    package_manifest.to_path_buf()
}

fn manifest_has_workspace_table(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "[workspace]" || trimmed.starts_with("[workspace.")
    })
}

fn metadata_memo() -> &'static Mutex<HashMap<PathBuf, Vec<CargoRoot>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Vec<CargoRoot>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load_workspace_roots(workspace_manifest: &Path) -> Result<Vec<CargoRoot>, RoleBuildError> {
    let key = crate::rust_include::canonical_path(workspace_manifest);
    if let Some(hit) = metadata_memo()
        .lock()
        .expect("cargo metadata cache")
        .get(&key)
        .cloned()
    {
        return Ok(hit);
    }
    let roots = load_workspace_roots_uncached(workspace_manifest)?;
    metadata_memo()
        .lock()
        .expect("cargo metadata cache")
        .insert(key, roots.clone());
    Ok(roots)
}

fn load_workspace_roots_uncached(
    workspace_manifest: &Path,
) -> Result<Vec<CargoRoot>, RoleBuildError> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(workspace_manifest)
        .output()
        .map_err(|err| cargo_err(workspace_manifest, &err.to_string()))?;
    if !output.status.success() {
        return Err(cargo_err(
            workspace_manifest,
            &String::from_utf8_lossy(&output.stderr),
        ));
    }
    let meta: Metadata = serde_json::from_slice(&output.stdout)
        .map_err(|err| cargo_err(workspace_manifest, &err.to_string()))?;
    let workspace = crate::rust_include::canonical_path(&meta.workspace_root);
    let mut roots = Vec::new();
    for pkg in meta.packages {
        let manifest_path = crate::rust_include::canonical_path(&pkg.manifest_path);
        for target in pkg.targets {
            roots.push(target_root(&workspace, &pkg.name, &manifest_path, target));
        }
    }
    Ok(roots)
}

fn target_root(
    workspace: &Path,
    package: &str,
    manifest_path: &Path,
    target: MetaTarget,
) -> CargoRoot {
    let mut kinds = target.kind;
    kinds.sort();
    let (_, allow_production) = target_contexts(&kinds);
    CargoRoot {
        src_path: crate::rust_include::canonical_path(&target.src_path),
        allow_production,
        workspace: workspace.to_path_buf(),
        package: package.to_string(),
        kinds,
        manifest_path: manifest_path.to_path_buf(),
    }
}

fn target_contexts(kinds: &[String]) -> (CodeContextSet, bool) {
    if kinds.is_empty() {
        return (CodeContextSet::production_only(), true);
    }
    if kinds.iter().all(|kind| kind == "test" || kind == "bench") {
        return (CodeContextSet::test_only(), false);
    }
    if kinds.iter().all(|kind| kind == "custom-build") {
        return (CodeContextSet::production_only(), true);
    }
    (CodeContextSet::both(), true)
}

pub fn cargo_entry_src_paths(files: &[PathBuf]) -> HashSet<PathBuf> {
    let Ok((roots, _)) = cargo_roots_for_files(files) else {
        return HashSet::new();
    };
    roots
        .into_iter()
        .filter(|root| {
            root.kinds
                .iter()
                .any(|kind| matches!(kind.as_str(), "bin" | "example" | "custom-build"))
        })
        .map(|root| root.src_path)
        .collect()
}

pub fn workspace_roots_at(repo_root: &Path) -> Result<Vec<CargoRoot>, RoleBuildError> {
    let manifest = repo_root.join("Cargo.toml");
    if !manifest.is_file() {
        return Ok(Vec::new());
    }
    load_workspace_roots(&manifest)
}

fn cargo_err(path: &Path, message: &str) -> RoleBuildError {
    RoleBuildError::CargoMetadata {
        workspace: path.to_path_buf(),
        message: message.trim().to_string(),
    }
}

#[cfg(test)]
mod cargo_test {
    use super::*;

    #[test]
    fn target_kind_classification() {
        let (ctx, allow) = target_contexts(&["test".into()]);
        assert!(ctx.is_test_only());
        assert!(!allow);
        let (ctx, allow) = target_contexts(&["lib".into()]);
        assert!(ctx.production && ctx.test);
        assert!(allow);
        let (ctx, allow) = target_contexts(&["custom-build".into()]);
        assert!(ctx.production && !ctx.test);
        assert!(allow);
        let (ctx, allow) = target_contexts(&[]);
        assert!(ctx.production);
        assert!(allow);
    }

    #[test]
    fn nearest_manifest_walks_parents() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname=\"t\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let file = src.join("lib.rs");
        std::fs::write(&file, "pub fn f() {}\n").unwrap();
        let found = nearest_manifest(&file).unwrap();
        assert!(found.ends_with("Cargo.toml"));
    }

    #[test]
    fn workspace_manifest_walks_to_workspace_table() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crate\"]\n",
        )
        .unwrap();
        let crate_dir = tmp.path().join("crate");
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        let pkg = crate_dir.join("Cargo.toml");
        std::fs::write(&pkg, "[package]\nname=\"c\"\nversion=\"0.1.0\"\n").unwrap();
        let found = workspace_manifest_for(&pkg);
        assert_eq!(found, tmp.path().join("Cargo.toml"));
    }
}
