const FORBIDDEN_FLAGS: &[&str] = &["-x", "--maxfail", "--lf", "--ff", "-n"];

pub fn validate_pytest_extra(extra: &[String]) -> Result<(), String> {
    for arg in extra {
        let flag = arg.split('=').next().unwrap_or(arg);
        if FORBIDDEN_FLAGS.contains(&flag) {
            return Err(format!(
                "pytest flag '{flag}' is incompatible with per-nodeid fork execution (forbidden: -x, --maxfail, --lf, --ff, -n)"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_incompatible_pytest_flags() {
        for flag in ["-x", "--maxfail=1", "--lf", "--ff", "-n"] {
            let err = validate_pytest_extra(&[flag.to_string()]).unwrap_err();
            assert!(err.contains("incompatible"), "{err}");
        }
        let err = validate_pytest_extra(&["-n".to_string(), "4".to_string()]).unwrap_err();
        assert!(err.contains("incompatible"), "{err}");
        assert!(
            validate_pytest_extra(&[
                "--tb=short".to_string(),
                "-m".to_string(),
                "slow".to_string(),
                "4".to_string(),
            ])
            .is_ok()
        );
    }
}
