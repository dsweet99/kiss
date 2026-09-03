use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn python_rslip_cache_root_for_repo(repo: &Path) -> PathBuf {
    let machine_id = fs::read("/etc/machine-id").unwrap();
    let host = hex_encode(trim_outer_ascii_whitespace(&machine_id));
    repo.join(".kiss")
        .join("rslip_cache")
        .join("hosts")
        .join(host)
}

pub(super) fn python_entries_fingerprint(cache_root: &Path) -> String {
    let mut h = python_fnv1a64(
        0xcbf2_9ce4_8422_2325,
        kiss::rslip::CACHE_SCHEMA_VERSION.as_bytes(),
    );
    for path in kiss::json_entry_paths(cache_root) {
        let meta = fs::metadata(&path).unwrap();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        for bytes in [
            name.as_bytes().to_vec(),
            meta.len().to_string().into_bytes(),
            modified_nanos(&meta).unwrap_or_default().into_bytes(),
        ] {
            h = python_fnv1a64(h, &bytes);
            h = python_fnv1a64(h, &[0]);
        }
    }
    format!("{h:016x}")
}

pub(super) fn python_source_input_fingerprint(root: &Path) -> String {
    let mut h = python_fnv1a64(
        0xcbf2_9ce4_8422_2325,
        kiss::rslip::CACHE_SCHEMA_VERSION.as_bytes(),
    );
    h = python_fnv1a64(h, b"python-workspace-inputs-v1");
    for path in python_source_input_paths(root) {
        let rel = path.strip_prefix(root).unwrap_or(&path);
        for bytes in [
            rel.to_string_lossy().as_bytes().to_vec(),
            fs::read(path).unwrap(),
        ] {
            h = python_fnv1a64(h, &bytes);
            h = python_fnv1a64(h, &[0]);
        }
    }
    format!("{h:016x}")
}

fn modified_nanos(meta: &fs::Metadata) -> Option<String> {
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_nanos().to_string())
}

fn python_source_input_paths(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    visit_python_source_inputs(root, &mut out);
    out.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    out
}

fn visit_python_source_inputs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let file_type = entry.file_type().unwrap();
        if file_type.is_dir() {
            if should_skip_python_source_input_dir(&path) {
                continue;
            }
            visit_python_source_inputs(&path, out);
        } else if file_type.is_file()
            && is_python_source_input_path(&path)
            && !is_python_test_module_path(&path)
        {
            out.push(path);
        }
    }
}

fn should_skip_python_source_input_dir(path: &Path) -> bool {
    kiss::rslip::should_skip_rslip_dir(path)
}

fn is_python_source_input_path(path: &Path) -> bool {
    kiss::rslip::is_rslip_cache_input(path)
}

fn is_python_test_module_path(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.ends_with(".py") && (name.starts_with("test_") || name.ends_with("_test.py"))
}

fn trim_outer_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &bytes[start..end]
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn python_fnv1a64(h: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0100_0000_01b3;
    bytes
        .iter()
        .fold(h, |acc, byte| (acc ^ u64::from(*byte)).wrapping_mul(PRIME))
}
