use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

pub(crate) fn is_workspace_rust_selector_file(
    path: &Path,
    member_manifest_dirs: &BTreeSet<PathBuf>,
) -> bool {
    is_workspace_rust_selector_file_cached(path, member_manifest_dirs, &mut HashMap::new())
}

pub(crate) fn is_workspace_rust_selector_file_cached(
    path: &Path,
    member_manifest_dirs: &BTreeSet<PathBuf>,
    nearest_manifest_cache: &mut HashMap<PathBuf, Option<PathBuf>>,
) -> bool {
    !is_rust_selector_fixture_path(path)
        && nearest_cargo_manifest_dir_cached(path, nearest_manifest_cache)
            .is_some_and(|dir| member_manifest_dirs.contains(&dir))
}

pub(crate) fn non_member_rust_crate_roots(
    repo_root: &Path,
    rust_paths: &[PathBuf],
) -> Result<Vec<String>, String> {
    if rust_paths.is_empty() {
        return Ok(Vec::new());
    }
    let member_manifest_dirs = cargo_workspace_member_manifest_dirs(repo_root)?;
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let mut roots = BTreeSet::new();
    for path in rust_paths {
        if is_workspace_rust_selector_file(path, &member_manifest_dirs) {
            continue;
        }
        let Some(manifest_dir) = nearest_cargo_manifest_dir(path) else {
            continue;
        };
        if !member_manifest_dirs.contains(&manifest_dir) {
            let rel = manifest_dir
                .strip_prefix(&root)
                .unwrap_or(&manifest_dir)
                .to_string_lossy()
                .replace('\\', "/");
            if !rel.is_empty() {
                roots.insert(rel);
            }
        }
    }
    Ok(roots.into_iter().collect())
}

pub(crate) fn cargo_workspace_member_manifest_dirs(
    repo_root: &Path,
) -> Result<BTreeSet<PathBuf>, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("error: kiss test: failed to run cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "error: kiss test: cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let metadata: CargoMetadataForSelectors = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("error: kiss test: failed to parse cargo metadata: {e}"))?;
    Ok(metadata
        .packages
        .into_iter()
        .filter(|pkg| metadata.workspace_members.contains(&pkg.id))
        .filter_map(|pkg| pkg.manifest_path.parent().map(canonical_manifest_dir))
        .collect())
}

fn is_rust_selector_fixture_path(path: &Path) -> bool {
    let mut saw_tests = false;
    for component in path.components() {
        let part = component.as_os_str().to_string_lossy();
        if saw_tests && (part == "fixtures" || part == "fake_rust") {
            return true;
        }
        if part == "tests" {
            saw_tests = true;
        }
    }
    false
}

#[derive(Deserialize)]
struct CargoMetadataForSelectors {
    packages: Vec<CargoPackageForSelectors>,
    #[serde(default)]
    workspace_members: BTreeSet<String>,
}

#[derive(Deserialize)]
struct CargoPackageForSelectors {
    id: String,
    manifest_path: PathBuf,
}

fn canonical_manifest_dir(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn nearest_cargo_manifest_dir(path: &Path) -> Option<PathBuf> {
    nearest_cargo_manifest_dir_cached(path, &mut HashMap::new())
}

fn nearest_cargo_manifest_dir_cached(
    path: &Path,
    cache: &mut HashMap<PathBuf, Option<PathBuf>>,
) -> Option<PathBuf> {
    let start = path.parent()?.to_path_buf();
    if let Some(hit) = cache.get(&start) {
        return hit.clone();
    }
    let mut dir = start.as_path();
    let mut walked = Vec::new();
    let found = loop {
        walked.push(dir.to_path_buf());
        if dir.join("Cargo.toml").is_file() {
            break Some(canonical_manifest_dir(dir));
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break None,
        }
    };
    for d in walked {
        cache.insert(d, found.clone());
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_path_filter_only_applies_under_tests() {
        assert!(is_rust_selector_fixture_path(Path::new(
            "repo/tests/fixtures/case.rs"
        )));
        assert!(is_rust_selector_fixture_path(Path::new(
            "repo/tests/fake_rust/case.rs"
        )));
        assert!(!is_rust_selector_fixture_path(Path::new(
            "repo/src/fixture_helpers/case.rs"
        )));
        assert!(!is_rust_selector_fixture_path(Path::new(
            "repo/tests/case.rs"
        )));
    }

    #[test]
    fn workspace_selector_file_requires_nearest_member_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let member = tmp.path().join("member");
        let nested = member.join("tests").join("fixtures").join("inner");
        std::fs::create_dir_all(member.join("src")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(member.join("Cargo.toml"), "").unwrap();
        std::fs::write(member.join("src").join("lib.rs"), "").unwrap();
        std::fs::write(nested.join("case.rs"), "").unwrap();

        let members = BTreeSet::from([canonical_manifest_dir(&member)]);

        assert!(is_workspace_rust_selector_file(
            &member.join("src").join("lib.rs"),
            &members
        ));
        assert!(!is_workspace_rust_selector_file(
            &nested.join("case.rs"),
            &members
        ));
        assert!(!is_workspace_rust_selector_file(
            &tmp.path().join("outside.rs"),
            &members
        ));
    }

    #[test]
    fn non_member_rust_crate_roots_lists_nested_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let member = tmp.path().join("member");
        let nested = tmp.path().join("nested");
        std::fs::create_dir_all(member.join("src")).unwrap();
        std::fs::create_dir_all(nested.join("src")).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::write(
            member.join("Cargo.toml"),
            "[package]\nname='member'\nversion='0.1.0'\nedition='2024'\n",
        )
        .unwrap();
        std::fs::write(member.join("src").join("lib.rs"), "pub fn m() {}\n").unwrap();
        std::fs::write(
            nested.join("Cargo.toml"),
            "[package]\nname='nested'\nversion='0.1.0'\nedition='2024'\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(nested.join("src").join("lib.rs"), "pub fn n() {}\n").unwrap();

        let roots = non_member_rust_crate_roots(
            tmp.path(),
            &[
                member.join("src").join("lib.rs"),
                nested.join("src").join("lib.rs"),
            ],
        )
        .unwrap();
        assert_eq!(roots, vec!["nested".to_string()]);
    }
}
