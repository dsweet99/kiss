use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

pub fn rust_cov_cache_tmp_parent(cache_root: &Path) -> PathBuf {
    std::env::temp_dir()
        .join("kiss-rust-llvm-cov")
        .join(format!("cache-{}", cache_root_digest(cache_root)))
}

pub(crate) fn cache_root_digest(cache_root: &Path) -> String {
    let canonical = fs::canonicalize(cache_root).unwrap_or_else(|_| cache_root.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(os_str_bytes(canonical.as_os_str()));
    hex_lower(&hasher.finalize())
}

#[cfg(unix)]
pub(crate) fn os_str_bytes(value: &OsStr) -> Vec<u8> {
    value.as_bytes().to_vec()
}

#[cfg(not(unix))]
pub(crate) fn os_str_bytes(value: &OsStr) -> Vec<u8> {
    value.to_string_lossy().as_bytes().to_vec()
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}
