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
