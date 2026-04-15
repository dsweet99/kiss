use std::path::Path;

use crate::config::error::ConfigError;
use crate::config::types::{Config, ConfigLanguage};

impl Config {
    fn load_config_chain(base: Self, lang: Option<ConfigLanguage>) -> Self {
        let mut config = base;
        if let Some(home) = std::env::var_os("HOME")
            && let Ok(content) = std::fs::read_to_string(Path::new(&home).join(".kissconfig"))
        {
            config.merge_from_toml(&content, lang);
        }
        if let Ok(content) = std::fs::read_to_string(".kissconfig") {
            config.merge_from_toml(&content, lang);
        }
        config
    }

    pub fn load() -> Self {
        Self::load_config_chain(Self::default(), None)
    }

    pub fn load_for_language(lang: ConfigLanguage) -> Self {
        let base = match lang {
            ConfigLanguage::Python => Self::python_defaults(),
            ConfigLanguage::Rust => Self::rust_defaults(),
        };
        Self::load_config_chain(base, Some(lang))
    }

    pub fn load_from(path: &Path) -> Self {
        let mut config = Self::default();
        if let Ok(content) = std::fs::read_to_string(path) {
            config.merge_from_toml(&content, None);
        } else {
            eprintln!("Warning: Could not read config file: {}", path.display());
        }
        config
    }

    pub fn load_from_for_language(path: &Path, lang: ConfigLanguage) -> Self {
        let mut config = match lang {
            ConfigLanguage::Python => Self::python_defaults(),
            ConfigLanguage::Rust => Self::rust_defaults(),
        };
        if let Ok(content) = std::fs::read_to_string(path) {
            config.merge_from_toml_with_path(&content, Some(lang), Some(path));
        } else {
            eprintln!("Warning: Could not read config file: {}", path.display());
        }
        config
    }

    pub fn load_from_content(content: &str, lang: ConfigLanguage) -> Self {
        let mut config = match lang {
            ConfigLanguage::Python => Self::python_defaults(),
            ConfigLanguage::Rust => Self::rust_defaults(),
        };
        config.merge_from_toml(content, Some(lang));
        config
    }

    /// Try to load config from a file, returning an error on failure.
    ///
    /// This is the Result-based API for library embedding. Unlike `load_from`,
    /// this function returns errors instead of printing to stderr.
    pub fn try_load_from(path: &Path, lang: ConfigLanguage) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::IoError {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Self::try_load_from_content(&content, lang)
    }

    /// Try to load config from TOML content, returning an error on failure.
    ///
    /// This is the Result-based API for library embedding. Unlike `load_from_content`,
    /// this function returns errors instead of printing to stderr.
    pub fn try_load_from_content(content: &str, lang: ConfigLanguage) -> Result<Self, ConfigError> {
        let mut config = match lang {
            ConfigLanguage::Python => Self::python_defaults(),
            ConfigLanguage::Rust => Self::rust_defaults(),
        };
        config.try_merge_from_toml(content, Some(lang))?;
        Ok(config)
    }
}
