use std::path::Path;

#[cfg(unix)]
pub fn set_sigpipe_default() {
    // When `kiss` output is piped (e.g. `kiss stats --all . | head`), downstream may close the pipe early.
    // Rust's default SIGPIPE behavior is "ignore", which turns this into an EPIPE write error and can panic.
    // Restoring SIGPIPE's default behavior makes the process terminate quietly instead of panicking.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
pub fn set_sigpipe_default() {}

/// Directory or filename prefixes excluded from `kiss check` by default.
/// Intentionally-violating fixtures live under `tests/fake_python/` and
/// `tests/fake_rust/`; they are analyzed directly in integration tests but
/// must not fail the repo's own quality gate.
pub const DEFAULT_CHECK_IGNORE_PREFIXES: &[&str] = &["fake_"];

pub fn default_check_ignore_prefixes() -> Vec<String> {
    DEFAULT_CHECK_IGNORE_PREFIXES
        .iter()
        .map(|prefix| (*prefix).to_string())
        .collect()
}

pub fn merge_check_ignore_prefixes(user: &[String]) -> Vec<String> {
    let mut ignore = default_check_ignore_prefixes();
    ignore.extend(user.iter().cloned());
    kiss::normalize_ignore_prefixes(&ignore)
}

pub fn validate_paths(paths: &[String]) {
    for p in paths {
        if !Path::new(p).exists() {
            eprintln!("Error: Path does not exist: {p}");
            std::process::exit(1);
        }
    }
}

pub fn validate_min_similarity(value: f64) -> Result<(), String> {
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "min_similarity must be within [0.0, 1.0], got {value}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{set_sigpipe_default, validate_min_similarity};

    #[test]
    fn set_sigpipe_default_is_callable() {
        set_sigpipe_default();
    }

    #[test]
    fn validate_min_similarity_accepts_endpoints() {
        assert!(validate_min_similarity(0.0).is_ok());
        assert!(validate_min_similarity(1.0).is_ok());
        assert!(validate_min_similarity(0.5).is_ok());
    }

    #[test]
    fn validate_min_similarity_rejects_out_of_range() {
        assert!(validate_min_similarity(-0.1).is_err());
        assert!(validate_min_similarity(1.5).is_err());
    }
}
