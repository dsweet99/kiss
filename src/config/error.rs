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
        match self {
            Self::UnknownKey { key, section } => {
                write!(f, "Unknown config key '{key}' in [{section}]")
            }
            Self::UnknownSection { section, hint } => {
                write!(f, "Unknown config section '[{section}]'")?;
                if let Some(h) = hint {
                    write!(f, " - did you mean '[{h}]'?")?;
                }
                Ok(())
            }
            Self::InvalidValue { key, message } => {
                write!(f, "Invalid value for '{key}': {message}")
            }
            Self::ParseError { message } => {
                write!(f, "Failed to parse config: {message}")
            }
            Self::IoError { path, message } => {
                write!(f, "Failed to read config '{path}': {message}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}
