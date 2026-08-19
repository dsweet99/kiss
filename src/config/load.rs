use std::path::Path;

use crate::config::error::ConfigError;
use crate::config::types::{Config, ConfigLanguage};

impl Config {
    fn load_config_chain(base: Self, lang: Option<ConfigLanguage>) -> Self {
        let mut config = base;
        if let Ok(content) = std::fs::read_to_string(super::kissconfig_path_from_cwd()) {
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

    pub fn load_for_language_with_override(path: &Path, lang: ConfigLanguage) -> Self {
        let mut config = Self::load_for_language(lang);
        match std::fs::read_to_string(path) {
            Ok(content) => {
                config.merge_from_toml_with_path(&content, Some(lang), Some(path));
            }
            Err(e) => {
                eprintln!("Warning: Config file not found ({}): {e}", path.display());
            }
        }
        config
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

    pub fn try_load_from(path: &Path, lang: ConfigLanguage) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::IoError {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Self::try_load_from_content(&content, lang)
    }

    pub fn try_load_from_content(content: &str, lang: ConfigLanguage) -> Result<Self, ConfigError> {
        let mut config = match lang {
            ConfigLanguage::Python => Self::python_defaults(),
            ConfigLanguage::Rust => Self::rust_defaults(),
        };
        config.try_merge_from_toml(content, Some(lang))?;
        Ok(config)
    }
}
