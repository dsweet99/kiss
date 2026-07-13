use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::RustLlvmCovError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspacePackage {
    pub id: String,
    pub name: String,
    pub manifest_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub struct WorkspaceMetadata {
    packages: Vec<WorkspacePackageRecord>,
    local_dep_ids: HashMap<String, BTreeSet<String>>,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkspacePackageRecord {
    package: WorkspacePackage,
    has_custom_build: bool,
    has_proc_macro: bool,
}

#[derive(serde::Deserialize)]
pub(crate) struct CargoMetadata {
    packages: Vec<CargoMetadataPackage>,
    #[serde(default)]
    workspace_members: Vec<String>,
    #[serde(default)]
    workspace_root: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct CargoMetadataPackage {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) manifest_path: String,
    #[serde(default)]
    targets: Vec<CargoMetadataTarget>,
    #[serde(default)]
    dependencies: Vec<CargoMetadataDependency>,
}

#[derive(serde::Deserialize, Default)]
pub(crate) struct CargoMetadataTarget {
    #[serde(default)]
    kind: Vec<String>,
    #[serde(default)]
    crate_types: Vec<String>,
}

#[derive(serde::Deserialize, Default)]
pub(crate) struct CargoMetadataDependency {
    #[allow(dead_code)]
    name: String,
    #[serde(default)]
    path: Option<String>,
}

impl CargoMetadata {
    pub(crate) fn workspace_root_path(&self) -> Option<&str> {
        self.workspace_root.as_deref()
    }

    pub(crate) fn workspace_member_ids(&self) -> &[String] {
        &self.workspace_members
    }

    #[allow(dead_code)]
    pub(crate) fn packages(&self) -> &[CargoMetadataPackage] {
        &self.packages
    }

    pub(crate) fn workspace_packages(&self) -> Vec<WorkspacePackage> {
        self.packages
            .iter()
            .map(|pkg| WorkspacePackage {
                id: pkg.id.clone(),
                name: pkg.name.clone(),
                manifest_dir: PathBuf::from(&pkg.manifest_path)
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from(".")),
            })
            .collect()
    }

    pub(crate) fn current_package_id(&self, cwd: &Path, cargo_args: &[String]) -> Option<String> {
        let manifest_path = effective_manifest_path(cwd, cargo_args);
        let manifest = manifest_path
            .canonicalize()
            .unwrap_or(manifest_path)
            .to_string_lossy()
            .to_string();
        self.packages.iter().find_map(|pkg| {
            let package_manifest = PathBuf::from(&pkg.manifest_path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&pkg.manifest_path))
                .to_string_lossy()
                .to_string();
            (package_manifest == manifest).then(|| pkg.id.clone())
        })
    }
}

pub(crate) fn load_cargo_metadata(
    cwd: &Path,
    cargo: &Path,
    cargo_args: &[String],
) -> Result<CargoMetadata, RustLlvmCovError> {
    let manifest_path = effective_manifest_path(cwd, cargo_args);
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&manifest_path)
        .current_dir(cwd)
        .output()
        .map_err(RustLlvmCovError::Io)?;
    if !output.status.success() {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "cargo metadata failed in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(|err| {
        RustLlvmCovError::InvalidRequest(format!("cargo metadata json parse failed: {err}"))
    })
}

pub(crate) fn workspace_metadata_from_cargo(
    cwd: &Path,
    cargo: &Path,
    cargo_args: &[String],
) -> Result<WorkspaceMetadata, RustLlvmCovError> {
    let metadata = load_cargo_metadata(cwd, cargo, cargo_args)?;
    Ok(WorkspaceMetadata::from_cargo_metadata(&metadata))
}

impl WorkspaceMetadata {
    pub(crate) fn from_cargo_metadata(metadata: &CargoMetadata) -> Self {
        let mut packages = Vec::new();
        for pkg in &metadata.packages {
            packages.push(WorkspacePackageRecord {
                package: WorkspacePackage {
                    id: pkg.id.clone(),
                    name: pkg.name.clone(),
                    manifest_dir: PathBuf::from(&pkg.manifest_path)
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| PathBuf::from(".")),
                },
                has_custom_build: pkg
                    .targets
                    .iter()
                    .any(|target| target.kind.iter().any(|kind| kind == "custom-build")),
                has_proc_macro: pkg.targets.iter().any(|target| {
                    target
                        .crate_types
                        .iter()
                        .any(|crate_type| crate_type == "proc-macro")
                }),
            });
        }
        let mut manifest_to_id = HashMap::new();
        for pkg in &metadata.packages {
            let manifest = PathBuf::from(&pkg.manifest_path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&pkg.manifest_path));
            manifest_to_id.insert(manifest, pkg.id.clone());
        }
        let mut local_dep_ids = HashMap::new();
        for pkg in &metadata.packages {
            let mut deps = BTreeSet::new();
            for dep in &pkg.dependencies {
                let Some(path) = dep.path.as_deref() else {
                    continue;
                };
                let dep_manifest = PathBuf::from(path)
                    .join("Cargo.toml")
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(path).join("Cargo.toml"));
                if let Some(dep_id) = manifest_to_id.get(&dep_manifest) {
                    deps.insert(dep_id.clone());
                }
            }
            local_dep_ids.insert(pkg.id.clone(), deps);
        }
        Self {
            packages,
            local_dep_ids,
        }
    }

    pub(crate) fn compile_time_package_ids(&self) -> BTreeSet<String> {
        let mut seeds = BTreeSet::new();
        for record in &self.packages {
            if record.has_custom_build || record.has_proc_macro {
                seeds.insert(record.package.id.clone());
            }
        }
        let mut closure = seeds.clone();
        let mut queue: VecDeque<_> = seeds.into_iter().collect();
        while let Some(pkg_id) = queue.pop_front() {
            if let Some(deps) = self.local_dep_ids.get(&pkg_id) {
                for dep_id in deps {
                    if closure.insert(dep_id.clone()) {
                        queue.push_back(dep_id.clone());
                    }
                }
            }
        }
        closure
    }

    /// Returns `None` when the `.rs` path cannot be classified conservatively.
    pub(crate) fn rs_compile_time_classification(
        &self,
        source_root: &Path,
        file: &Path,
    ) -> Option<bool> {
        if !file
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        {
            return Some(false);
        }
        let root = source_root
            .canonicalize()
            .unwrap_or_else(|_| source_root.to_path_buf());
        let absolute = if file.is_absolute() {
            file.canonicalize().unwrap_or_else(|_| file.to_path_buf())
        } else {
            root.join(file)
                .canonicalize()
                .unwrap_or_else(|_| root.join(file))
        };
        let rel = absolute.strip_prefix(&root).ok()?;
        if rel.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        }) {
            return None;
        }
        let mut matches = Vec::new();
        for record in &self.packages {
            let manifest_dir = record
                .package
                .manifest_dir
                .canonicalize()
                .unwrap_or_else(|_| record.package.manifest_dir.clone());
            if absolute.starts_with(&manifest_dir) {
                matches.push(record.package.id.clone());
            }
        }
        match matches.len() {
            0 => None,
            1 => {
                let compile_time = self.compile_time_package_ids();
                Some(compile_time.contains(&matches[0]))
            }
            _ => None,
        }
    }
}

pub(crate) fn effective_manifest_path(cwd: &Path, cargo_args: &[String]) -> PathBuf {
    let mut index = 0usize;
    while index < cargo_args.len() {
        match cargo_args[index].as_str() {
            "--manifest-path" => {
                if let Some(value) = cargo_args.get(index + 1) {
                    return PathBuf::from(value);
                }
            }
            _ => {
                if let Some(value) = cargo_args[index].strip_prefix("--manifest-path=") {
                    return PathBuf::from(value);
                }
            }
        }
        index += 1;
    }
    cwd.join("Cargo.toml")
}

#[cfg(test)]
pub(crate) fn cargo_metadata_witness_for_test() -> CargoMetadata {
    CargoMetadata {
        packages: vec![CargoMetadataPackage {
            id: "pkg-id".to_string(),
            name: "pkg".to_string(),
            manifest_path: "/repo/Cargo.toml".to_string(),
            targets: vec![],
            dependencies: vec![],
        }],
        workspace_members: vec!["pkg-id".to_string()],
        workspace_root: Some("/repo".to_string()),
    }
}

#[cfg(test)]
pub(crate) fn workspace_package_for_test(
    id: &str,
    name: &str,
    manifest_dir: PathBuf,
) -> WorkspacePackage {
    WorkspacePackage {
        id: id.to_string(),
        name: name.to_string(),
        manifest_dir,
    }
}

#[cfg(test)]
#[path = "cargo_workspace_metadata_test.rs"]
mod tests;

#[cfg(test)]
mod coverage_witness {
    use std::path::PathBuf;

    use super::{
        CargoMetadata, CargoMetadataDependency, CargoMetadataPackage, CargoMetadataTarget,
        WorkspaceMetadata, WorkspacePackageRecord, workspace_package_for_test,
    };

    #[test]
    fn witness_workspace_metadata_struct_fields() {
        let _ = std::mem::size_of::<WorkspacePackageRecord>();
        let _ = std::mem::size_of::<CargoMetadataTarget>();
        let _ = std::mem::size_of::<CargoMetadataDependency>();
        let package = workspace_package_for_test("pkg", "name", PathBuf::from("/repo"));
        assert_eq!(package.name, "name");
        let metadata = CargoMetadata {
            packages: vec![CargoMetadataPackage {
                id: "pkg".to_string(),
                name: "name".to_string(),
                manifest_path: "/repo/Cargo.toml".to_string(),
                targets: vec![serde_json::from_value(serde_json::json!({
                    "kind": ["lib"],
                    "crate_types": ["lib"]
                }))
                .unwrap()],
                dependencies: vec![serde_json::from_value(serde_json::json!({
                    "name": "dep",
                    "path": "/repo/dep"
                }))
                .unwrap()],
            }],
            workspace_members: vec!["pkg".to_string()],
            workspace_root: Some("/repo".to_string()),
        };
        let workspace = WorkspaceMetadata::from_cargo_metadata(&metadata);
        assert_eq!(workspace.compile_time_package_ids().len(), 0);
        let packages = metadata.workspace_packages();
        assert_eq!(packages[0].id, "pkg");
        assert_eq!(package.manifest_dir, PathBuf::from("/repo"));
    }
}
