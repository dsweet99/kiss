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

#[cfg(test)]
mod tests {
    use super::validate_supported_rust_test_args;

    #[test]
    fn validate_supported_rust_test_args_rejects_unknown_flags() {
        let err = validate_supported_rust_test_args(&["--test-threads".to_string()]).unwrap_err();
        assert!(err.contains("unsupported Rust test argument"));
    }
}
