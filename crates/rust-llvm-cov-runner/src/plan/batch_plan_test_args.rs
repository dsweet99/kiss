pub fn validate_supported_rust_test_args(test_args: &[String]) -> Result<(), String> {
    let mut index = 0;
    while index < test_args.len() {
        let arg = &test_args[index];
        match arg.as_str() {
            "--exact" | "--nocapture" | "--no-capture" | "--ignored" | "--include-ignored" => {
                index += 1;
            }
            "--skip" => {
                let Some(pattern) = test_args.get(index + 1) else {
                    return Err("--skip requires a non-empty pattern".to_string());
                };
                if pattern.is_empty() {
                    return Err("--skip requires a non-empty pattern".to_string());
                }
                index += 2;
            }
            _ if arg.starts_with("--skip=") && arg.len() > "--skip=".len() => {
                index += 1;
            }
            _ => {
                return Err(format!(
                    "unsupported Rust test argument `{arg}`; supported forms are --exact, --nocapture, --no-capture, --ignored, --include-ignored, and repeated --skip <pattern>"
                ));
            }
        }
    }
    Ok(())
}

#[must_use]
pub fn identity_relevant_test_args(test_args: &[String]) -> Vec<String> {
    test_args
        .iter()
        .filter(|arg| !matches!(arg.as_str(), "--nocapture" | "--no-capture"))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{identity_relevant_test_args, validate_supported_rust_test_args};

    #[test]
    fn validate_supported_rust_test_args_accepts_supported_forms() {
        validate_supported_rust_test_args(&[
            "--exact".to_string(),
            "--nocapture".to_string(),
            "--no-capture".to_string(),
            "--ignored".to_string(),
            "--include-ignored".to_string(),
            "--skip".to_string(),
            "slow".to_string(),
            "--skip=flaky".to_string(),
        ])
        .unwrap();
    }

    #[test]
    fn validate_supported_rust_test_args_rejects_unknown_flags() {
        let err = validate_supported_rust_test_args(&["--test-threads".to_string()]).unwrap_err();
        assert!(err.contains("unsupported Rust test argument"));
    }

    #[test]
    fn validate_supported_rust_test_args_rejects_empty_skip_patterns() {
        assert!(
            validate_supported_rust_test_args(&["--skip".to_string()])
                .unwrap_err()
                .contains("requires")
        );
        assert!(
            validate_supported_rust_test_args(&["--skip".to_string(), String::new()])
                .unwrap_err()
                .contains("requires")
        );
        assert!(validate_supported_rust_test_args(&["--skip=".to_string()]).is_err());
    }

    #[test]
    fn identity_relevant_test_args_drops_nocapture_flags() {
        assert_eq!(
            identity_relevant_test_args(&[
                "--exact".to_string(),
                "--nocapture".to_string(),
                "--no-capture".to_string(),
            ]),
            vec!["--exact".to_string()]
        );
    }
}
