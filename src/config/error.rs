/// Error type for configuration validation
#[derive(Debug, Clone)]
pub enum ConfigError {
    /// Unknown key in a config section
    UnknownKey { key: String, section: String },
    /// Unknown section in the config file
    UnknownSection {
        section: String,
        hint: Option<String>,
    },
    /// Invalid value for a config key
    InvalidValue { key: String, message: String },
    /// Failed to parse TOML content
    ParseError { message: String },
    /// Failed to read config file
    IoError { path: String, message: String },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_config_error(self, f)
    }
}

pub(crate) fn write_config_error(
    err: &ConfigError,
    f: &mut std::fmt::Formatter<'_>,
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
mod tests {
    use super::*;
    use std::fmt;

    struct ConfigErrorViaWriter<'a>(&'a ConfigError);

    impl fmt::Display for ConfigErrorViaWriter<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write_config_error(self.0, f)
        }
    }

    #[test]
    fn display_formats_each_config_error_variant() {
        let cases = [
            (
                ConfigError::UnknownKey {
                    key: "foo".into(),
                    section: "gate".into(),
                },
                "Unknown config key 'foo' in [gate]",
            ),
            (
                ConfigError::UnknownSection {
                    section: "gtae".into(),
                    hint: Some("gate".into()),
                },
                "Unknown config section '[gtae]' - did you mean '[gate]'?",
            ),
            (
                ConfigError::InvalidValue {
                    key: "min_similarity".into(),
                    message: "expected float".into(),
                },
                "Invalid value for 'min_similarity': expected float",
            ),
            (
                ConfigError::ParseError {
                    message: "bad toml".into(),
                },
                "Failed to parse config: bad toml",
            ),
            (
                ConfigError::IoError {
                    path: "kiss.toml".into(),
                    message: "missing".into(),
                },
                "Failed to read config 'kiss.toml': missing",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
            assert_eq!(ConfigErrorViaWriter(&err).to_string(), expected);
        }
    }

    #[test]
    fn unknown_section_without_hint_has_no_suggestion() {
        let err = ConfigError::UnknownSection {
            section: "extra".into(),
            hint: None,
        };
        assert_eq!(err.to_string(), "Unknown config section '[extra]'");
    }
}
