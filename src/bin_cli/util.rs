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

pub fn normalize_ignore_prefixes(prefixes: &[String]) -> Vec<String> {
    let result: Vec<String> = prefixes
        .iter()
        .map(|p| p.trim_end_matches('/').to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if result.iter().any(|p| p == ".") {
        eprintln!("Warning: --ignore '.' matches all files");
    }
    result
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
    use super::validate_min_similarity;

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
