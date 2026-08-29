use std::io;
use std::path::Path;

use crate::analyze_cache::fnv1a64;

use super::digest::hash_file_contents;

const CONFIG_FILES: &[&str] = &[
    "pytest.ini",
    "pyproject.toml",
    "setup.cfg",
    "tox.ini",
    ".kissconfig",
];

pub(super) fn mix_collection_inventory(
    repo_root: &Path,
    ignore: &[String],
    python_fp: &str,
) -> io::Result<String> {
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, b"python-collection-inventory-v1");
    h = fnv1a64(h, python_fp.as_bytes());
    h = hash_config_files(h, repo_root, ignore)?;
    for plugin in kiss::TestSectionConfig::load().pytest_plugins {
        h = fnv1a64(h, plugin.as_bytes());
        h = fnv1a64(h, &[0]);
    }
    Ok(format!("{h:016x}"))
}

fn hash_config_files(mut h: u64, repo_root: &Path, ignore: &[String]) -> io::Result<u64> {
    for name in CONFIG_FILES {
        if kiss::path_ignored_by_prefixes(name, ignore) {
            h = fnv1a64(h, name.as_bytes());
            h = fnv1a64(h, b"ignored");
            continue;
        }
        let path = repo_root.join(name);
        if path.is_file() {
            h = hash_file_contents(h, name, repo_root, &path)?;
        } else {
            h = fnv1a64(h, name.as_bytes());
            h = fnv1a64(h, b"absent");
        }
    }
    Ok(h)
}
