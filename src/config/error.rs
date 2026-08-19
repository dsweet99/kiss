#[derive(Debug, Clone)]
pub enum ConfigError {
    UnknownKey { key: String, section: String },
    UnknownSection {
        section: String,
        hint: Option<String>,
    },
    InvalidValue { key: String, message: String },
    ParseError { message: String },
    IoError { path: String, message: String },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_config_error(self, f)
    }
}

pub(crate) fn write_config_error(
    err: &ConfigError,
    f: &mut impl std::fmt::Write,
) -> std::fmt::Result {
    match err {
        ConfigError::UnknownKey { key, section } => {
            write!(f, "Unknown config key '{key}' in [{section}]")
        }
        ConfigError::UnknownSection { section, hint } => {
            write!(f, "Unknown config section '[{section}]'")?;
            if let Some(h) = hint {
                write!(f, " - did you mean '[{h}]'?")?;
            }
            Ok(())
        }
        ConfigError::InvalidValue { key, message } => {
            write!(f, "Invalid value for '{key}': {message}")
        }
        ConfigError::ParseError { message } => {
            write!(f, "Failed to parse config: {message}")
        }
        ConfigError::IoError { path, message } => {
            write!(f, "Failed to read config '{path}': {message}")
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl ConfigError {
        fn witness() -> Self {
            Self::ParseError {
                message: "test".into(),
            }
        }
    }

    #[test]
    fn witness_config_error_display() {
        for err in [
            ConfigError::witness(),
            ConfigError::UnknownKey {
                key: "k".into(),
                section: "s".into(),
            },
            ConfigError::UnknownSection {
                section: "s".into(),
                hint: None,
            },
            ConfigError::UnknownSection {
                section: "s".into(),
                hint: Some("hint".into()),
            },
            ConfigError::InvalidValue {
                key: "k".into(),
                message: "m".into(),
            },
            ConfigError::IoError {
                path: "p".into(),
                message: "m".into(),
            },
        ] {
            let mut direct = String::new();
            write_config_error(&err, &mut direct).unwrap();
            let display = format!("{err}");
            assert!(!direct.is_empty());
            assert_eq!(direct, display);
            assert_eq!(direct, err.to_string());
        }
    }
}
