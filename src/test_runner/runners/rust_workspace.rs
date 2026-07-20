use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

pub(super) fn is_workspace_rust_selector_file(
    path: &Path,
    member_manifest_dirs: &BTreeSet<PathBuf>,
) -> bool {
    !is_rust_selector_fixture_path(path)
        && nearest_cargo_manifest_dir(path).is_some_and(|dir| member_manifest_dirs.contains(&dir))
}

pub(super) fn cargo_workspace_member_manifest_dirs(
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
    let mut dir = path.parent()?;
    loop {
        if dir.join("Cargo.toml").is_file() {
            return Some(canonical_manifest_dir(dir));
        }
        dir = dir.parent()?;
    }
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
}
