use std::path::Path;

use rayon::prelude::*;

use super::error::RoleBuildError;

pub const ROLE_SCHEMA_VERSION: &str = "roles-v2";

fn fnv1a64_local(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub fn role_input_fingerprint(
    py_files: &[std::path::PathBuf],
    rs_files: &[std::path::PathBuf],
) -> Result<String, RoleBuildError> {
    let mut h = fnv1a64_local(0xcbf2_9ce4_8422_2325, ROLE_SCHEMA_VERSION.as_bytes());
    let mut paths: Vec<&std::path::PathBuf> = py_files.iter().chain(rs_files).collect();
    paths.sort();
    let metas: Vec<(String, Option<(u64, u128)>)> = paths
        .into_par_iter()
        .map(|path| {
            let key = path.to_string_lossy().into_owned();
            let meta = std::fs::metadata(path).ok().and_then(|meta| {
                let modified = meta.modified().ok()?;
                let d = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
                Some((meta.len(), d.as_nanos()))
            });
            (key, meta)
        })
        .collect();
    for (key, meta) in metas {
        h = fnv1a64_local(h, key.as_bytes());
        if let Some((len, nanos)) = meta {
            h = fnv1a64_local(h, &len.to_le_bytes());
            h = fnv1a64_local(h, &nanos.to_le_bytes());
        }
    }
    mix_cargo(h, rs_files)
}

fn mix_cargo(h: u64, rs_files: &[std::path::PathBuf]) -> Result<String, RoleBuildError> {
    let (roots, _) = super::rust_cargo::cargo_roots_for_files(rs_files)?;
    Ok(mix_manifests(mix_root_tuples(h, &roots), rs_files, &roots))
}

fn root_tuple(root: &super::rust_cargo::CargoRoot) -> String {
    format!(
        "{}:{}:{}:{}",
        root.workspace.display(),
        root.package,
        root.kinds.join(","),
        root.src_path.display()
    )
}

fn mix_manifests(
    mut h: u64,
    rs_files: &[std::path::PathBuf],
    roots: &[super::rust_cargo::CargoRoot],
) -> String {
    let mut manifests = Vec::new();
    for file in rs_files {
        if let Some(manifest) = nearest_manifest(file) {
            manifests.push(crate::rust_include::canonical_path(&manifest));
        }
    }
    for root in roots {
        manifests.push(root.workspace.join("Cargo.toml"));
        manifests.push(root.manifest_path.clone());
    }
    manifests.sort();
    manifests.dedup();
    for manifest in manifests {
        h = fnv1a64_local(h, manifest.to_string_lossy().as_bytes());
        if let Ok(bytes) = std::fs::read(&manifest) {
            h = fnv1a64_local(h, &bytes);
        }
    }
    format!("{h:016x}")
}

pub fn workspace_preflight_fingerprint(repo_root: &Path) -> Result<String, RoleBuildError> {
    let roots = super::rust_cargo::workspace_roots_at(repo_root)?;
    Ok(mix_manifests(mix_root_tuples(0, &roots), &[], &roots))
}

fn mix_root_tuples(mut h: u64, roots: &[super::rust_cargo::CargoRoot]) -> u64 {
    let mut tuples: Vec<String> = roots.iter().map(root_tuple).collect();
    tuples.sort();
    for tuple in tuples {
        h = fnv1a64_local(h, tuple.as_bytes());
        h = fnv1a64_local(h, &[0]);
    }
    h
}

fn nearest_manifest(path: &Path) -> Option<std::path::PathBuf> {
    for ancestor in path.ancestors() {
        let candidate = ancestor.join("Cargo.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod fingerprint_test {
    use super::*;

    #[test]
    fn schema_change_would_change_constant() {
        assert_eq!(ROLE_SCHEMA_VERSION, "roles-v2");
        let fp = role_input_fingerprint(&[], &[]).unwrap();
        assert_eq!(fp.len(), 16);
    }

    #[test]
    fn cargo_manifest_change_changes_fingerprint_before_parse() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let lib = src.join("lib.rs");
        std::fs::write(&lib, "pub fn f() {}\n").unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let before = role_input_fingerprint(&[], std::slice::from_ref(&lib)).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[[bin]]\nname = \"tool\"\npath = \"src/lib.rs\"\n",
        )
        .unwrap();
        let after = role_input_fingerprint(&[], std::slice::from_ref(&lib)).unwrap();
        assert_ne!(
            before, after,
            "Cargo target metadata must change the role fingerprint before parsing"
        );
    }
}
