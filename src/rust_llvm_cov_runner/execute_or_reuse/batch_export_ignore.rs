use std::path::{Path, PathBuf};

use crate::rust_llvm_cov_runner::RustLlvmCovError;
use crate::rust_llvm_cov_runner::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::{
    WorkspacePackage, load_cargo_metadata,
};

pub(crate) fn resolve_ignore_filename_regex(
    req: &RustCoverageBatchRequest,
    target_dir: &Path,
) -> Result<Option<String>, RustLlvmCovError> {
    let metadata = load_cargo_metadata(&req.cwd, &req.cargo, &req.cargo_args)?;
    let workspace_root = metadata
        .workspace_root_path()
        .map(PathBuf::from)
        .unwrap_or_else(|| req.cwd.clone());
    let packages = metadata.workspace_packages();
    if packages.is_empty() {
        return Ok(None);
    }
    let (workspace, package_filters, _) = parse_cargo_scope_args(&req.cargo_args);
    let current_package = metadata.current_package_id(&req.cwd, &req.cargo_args);
    let included = resolve_included_packages(
        &packages,
        metadata.workspace_member_ids(),
        current_package.as_deref(),
        workspace,
        &package_filters,
    )?;
    let excluded_dirs = packages
        .iter()
        .filter(|pkg| !included.iter().any(|name| name == &pkg.name))
        .map(|pkg| pkg.manifest_dir.clone())
        .collect::<Vec<_>>();
    Ok(Some(build_ignore_filename_regex(
        &workspace_root,
        target_dir,
        &excluded_dirs,
    )))
}

fn parse_cargo_scope_args(cargo_args: &[String]) -> (bool, Vec<String>, Vec<String>) {
    let mut workspace = false;
    let mut packages = Vec::new();
    let mut excluded = Vec::new();
    let mut index = 0usize;
    while index < cargo_args.len() {
        let arg = cargo_args[index].as_str();
        match arg {
            "--workspace" => workspace = true,
            "-p" | "--package" => {
                if let Some(value) = cargo_args.get(index + 1) {
                    packages.push(value.clone());
                    index += 1;
                }
            }
            "--exclude-from-report" => {
                if let Some(value) = cargo_args.get(index + 1) {
                    excluded.push(value.clone());
                    index += 1;
                }
            }
            _ if arg.starts_with("--package=") => {
                packages.push(
                    arg.split_once('=')
                        .map(|(_, v)| v)
                        .unwrap_or("")
                        .to_string(),
                );
            }
            _ => {}
        }
        index += 1;
    }
    (workspace, packages, excluded)
}

fn resolve_included_packages(
    packages: &[WorkspacePackage],
    workspace_members: &[String],
    current_package: Option<&str>,
    workspace: bool,
    package_filters: &[String],
) -> Result<Vec<String>, RustLlvmCovError> {
    let member_ids: Vec<&str> = if workspace_members.is_empty() {
        packages.iter().map(|pkg| pkg.id.as_str()).collect()
    } else {
        workspace_members.iter().map(String::as_str).collect()
    };
    let member_names: Vec<String> = member_ids
        .iter()
        .filter_map(|id| {
            packages
                .iter()
                .find(|pkg| pkg.id == *id)
                .map(|pkg| pkg.name.clone())
        })
        .collect();
    if workspace {
        return Ok(member_names);
    }
    if !package_filters.is_empty() {
        let mut included = Vec::new();
        for filter in package_filters {
            let matched = packages
                .iter()
                .filter(|pkg| package_name_matches(&pkg.name, filter))
                .map(|pkg| pkg.name.clone())
                .collect::<Vec<_>>();
            if matched.is_empty() {
                return Err(RustLlvmCovError::InvalidRequest(format!(
                    "cargo package filter `{filter}` did not match any workspace package"
                )));
            }
            included.extend(matched);
        }
        included.sort();
        included.dedup();
        return Ok(included);
    }
    if let Some(current) = current_package {
        let current_name = packages
            .iter()
            .find(|pkg| pkg.id == current)
            .map(|pkg| pkg.name.clone())
            .ok_or_else(|| {
                RustLlvmCovError::InvalidRequest(format!(
                    "cargo metadata root package not found for {current}"
                ))
            })?;
        return Ok(vec![current_name]);
    }
    Ok(member_names)
}

fn package_name_matches(package_name: &str, filter: &str) -> bool {
    package_name == filter || package_name.replace('_', "-") == filter.replace('_', "-")
}

fn build_ignore_filename_regex(
    workspace_root: &Path,
    target_dir: &Path,
    excluded_manifest_dirs: &[PathBuf],
) -> String {
    const SEPARATOR: &str = "/";
    let mut parts = Vec::new();
    let workspace = regex_escape_path(&workspace_root.to_string_lossy());
    parts.push(format!(
        "{SEPARATOR}rustc{SEPARATOR}([0-9a-f]+|[0-9]+\\.[0-9]+\\.[0-9]+){SEPARATOR}"
    ));
    parts.push(format!(
        "^{workspace}({SEPARATOR}.*)?{SEPARATOR}(tests|examples|benches){SEPARATOR}"
    ));
    parts.push(format!(
        "^{workspace}({SEPARATOR}.*)?{SEPARATOR}(tests\\.rs|[0-9a-zA-Z_-]+[_-]tests\\.rs)$"
    ));
    parts.push(abs_path_prefix_regex(target_dir));
    for dir in excluded_manifest_dirs {
        parts.push(abs_path_prefix_regex(dir));
    }
    parts.join("|")
}

fn abs_path_prefix_regex(path: &Path) -> String {
    let escaped = regex_escape_path(&path.to_string_lossy());
    format!("^{escaped}($|/)")
}

fn regex_escape_path(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '.' | '+' | '*' | '?' | '^' | '$' | '{' | '}' | '[' | ']' | '|' | '(' | ')' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
pub(crate) fn resolve_included_packages_for_test(
    packages: &[WorkspacePackage],
    workspace_members: &[String],
    current_package: Option<&str>,
    workspace: bool,
    package_filters: &[String],
) -> Result<Vec<String>, RustLlvmCovError> {
    resolve_included_packages(
        packages,
        workspace_members,
        current_package,
        workspace,
        package_filters,
    )
}

#[cfg(test)]
use crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::CargoMetadata;

#[cfg(test)]
pub(crate) use crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::{
    cargo_metadata_witness_for_test, effective_manifest_path as effective_manifest_path_for_test,
    workspace_package_for_test,
};

#[cfg(test)]
pub(crate) fn load_cargo_metadata_for_test(
    cwd: &Path,
    cargo: &Path,
    cargo_args: &[String],
) -> Result<CargoMetadata, RustLlvmCovError> {
    load_cargo_metadata(cwd, cargo, cargo_args)
}

#[cfg(test)]
pub(crate) fn ignore_filename_regex_for_workspace_packages(
    workspace_root: &Path,
    target_dir: &Path,
    packages: &[WorkspacePackage],
    workspace_members: &[String],
    current_package: Option<&str>,
    workspace: bool,
    package_filters: &[String],
) -> Result<Option<String>, RustLlvmCovError> {
    if packages.is_empty() {
        return Ok(None);
    }
    let included = resolve_included_packages(
        packages,
        workspace_members,
        current_package,
        workspace,
        package_filters,
    )?;
    let excluded_dirs = packages
        .iter()
        .filter(|pkg| !included.iter().any(|name| name == &pkg.name))
        .map(|pkg| pkg.manifest_dir.clone())
        .collect::<Vec<_>>();
    Ok(Some(build_ignore_filename_regex(
        workspace_root,
        target_dir,
        &excluded_dirs,
    )))
}

#[cfg(test)]
mod inline_coverage_tests {
    use super::*;

    #[test]
    fn witness_struct_fields_are_constructible() {
        let package = WorkspacePackage {
            id: "pkg-id".to_string(),
            name: "pkg".to_string(),
            manifest_dir: PathBuf::from("/repo"),
        };
        let metadata = cargo_metadata_witness_for_test();
        assert_eq!(
            metadata.workspace_packages()[0].manifest_dir,
            PathBuf::from("/repo")
        );
        assert_eq!(package.id, "pkg-id");
    }
}

#[cfg(test)]
#[path = "batch_export_ignore_test.rs"]
mod tests;
