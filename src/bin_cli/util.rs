use std::path::Path;

pub use kiss::reject_unconfigured_languages;

#[cfg(unix)]
pub fn set_sigpipe_default() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
pub fn set_sigpipe_default() {}

pub use kiss::merge_check_ignore_prefixes;

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
    use super::{merge_check_ignore_prefixes, set_sigpipe_default, validate_min_similarity};

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

    #[test]
    fn merge_check_ignore_prefixes_preserves_user_prefixes() {
        let merged = merge_check_ignore_prefixes(&["custom/".to_string()]);
        assert_eq!(merged, vec!["fake_", "fixtures", "custom"]);
    }
}
