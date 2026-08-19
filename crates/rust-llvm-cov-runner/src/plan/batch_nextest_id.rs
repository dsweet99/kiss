use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn libtest_binary_prefix(package_name: &str, target_name: &str) -> String {
    format!("{package_name}::{target_name}")
}

pub(crate) fn nextest_binary_id(package_name: &str, target_name: &str, kinds: &[String]) -> String {
    let kind_set: std::collections::BTreeSet<&str> = kinds.iter().map(String::as_str).collect();
    if kind_set.contains("bin") {
        return format!("{package_name}::bin/{target_name}");
    }
    if kind_set.contains("test") {
        return format!("{package_name}::{target_name}");
    }
    if kind_set.contains("example") {
        return format!("{package_name}::example/{target_name}");
    }
    if kind_set.contains("bench") {
        return format!("{package_name}::bench/{target_name}");
    }



    libtest_binary_prefix(package_name, target_name)
}

pub(crate) fn package_name_from_manifest(
    manifest_path: &Path,
    cache: &mut BTreeMap<PathBuf, String>,
) -> Option<String> {
    if let Some(name) = cache.get(manifest_path) {
        return Some(name.clone());
    }
    let text = fs::read_to_string(manifest_path).ok()?;
    let value: toml::Value = toml::from_str(&text).ok()?;
    let name = value
        .get("package")?
        .get("name")?
        .as_str()?
        .to_string();
    cache.insert(manifest_path.to_path_buf(), name.clone());
    Some(name)
}

pub(crate) fn prefer_deps_executable(existing: &str, candidate: &str) -> bool {
    let candidate_in_deps = Path::new(candidate)
        .parent()
        .and_then(|parent| parent.file_name())
        .is_some_and(|name| name == "deps");
    let existing_in_deps = Path::new(existing)
        .parent()
        .and_then(|parent| parent.file_name())
        .is_some_and(|name| name == "deps");
    candidate_in_deps && !existing_in_deps
}

#[cfg(test)]
mod tests {
    use super::{libtest_binary_prefix, nextest_binary_id, package_name_from_manifest, prefer_deps_executable};
    use std::collections::BTreeMap;
    use std::fs;

    #[test]
    fn nextest_binary_id_covers_lib_bin_and_integration() {
        assert_eq!(
            nextest_binary_id("kiss-ai", "kiss", &["lib".into()]),
            "kiss-ai::kiss"
        );
        assert_eq!(
            nextest_binary_id("rslip", "rslip", &["lib".into()]),
            "rslip::rslip"
        );
        assert_eq!(
            nextest_binary_id("kiss-ai", "kiss", &["bin".into()]),
            "kiss-ai::bin/kiss"
        );
        assert_eq!(
            nextest_binary_id("kiss-ai", "integration_suite", &["test".into()]),
            "kiss-ai::integration_suite"
        );
        assert_eq!(libtest_binary_prefix("kiss-ai", "kiss"), "kiss-ai::kiss");
    }

    #[test]
    fn package_name_reads_manifest_not_directory_name() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = tmp.path().join("Cargo.toml");
        fs::write(
            &manifest,
            "[package]\nname = \"kiss-ai\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let mut cache = BTreeMap::new();
        assert_eq!(
            package_name_from_manifest(&manifest, &mut cache).as_deref(),
            Some("kiss-ai")
        );
        assert_eq!(cache.len(), 1);
        assert_eq!(
            package_name_from_manifest(&manifest, &mut cache).as_deref(),
            Some("kiss-ai")
        );
    }

    #[test]
    fn prefer_deps_executable_selects_deps_path() {
        assert!(prefer_deps_executable(
            "/repo/target/debug/kiss",
            "/repo/target/debug/deps/kiss-abc"
        ));
        assert!(!prefer_deps_executable(
            "/repo/target/debug/deps/kiss-abc",
            "/repo/target/debug/kiss"
        ));
    }
}
