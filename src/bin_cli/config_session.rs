use kiss::{Config, ConfigLanguage, GateConfig};
use std::path::{Path, PathBuf};

pub fn ensure_default_config_exists() {
    let local_config = Path::new(".kissconfig");
    if local_config.exists() {
        return;
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home_config = Path::new(&home).join(".kissconfig");
        if !home_config.exists()
            && let Err(e) = std::fs::write(&home_config, kiss::default_config_toml())
        {
            eprintln!(
                "Note: Could not write default config to {}: {}",
                home_config.display(),
                e
            );
        }
    }
}

pub fn run_init_command(repo_path: &Path) -> i32 {
    if !repo_path.exists() {
        eprintln!("Error: Repo path does not exist: {}", repo_path.display());
        return 1;
    }
    if !repo_path.is_dir() {
        eprintln!(
            "Error: Repo path is not a directory: {}",
            repo_path.display()
        );
        return 1;
    }

    let config_path = repo_path.join(".kissconfig");
    if config_path.exists() {
        println!(
            "Skipped writing {} because it already exists; did not overwrite it.",
            config_path.display()
        );
        return 0;
    }

    match std::fs::write(&config_path, kiss::default_config_toml()) {
        Ok(()) => {
            println!("Wrote default config to {}", config_path.display());
            0
        }
        Err(e) => {
            eprintln!("Error: Could not write {}: {}", config_path.display(), e);
            1
        }
    }
}

pub fn load_gate_config(config_path: Option<&PathBuf>, use_defaults: bool) -> GateConfig {
    if use_defaults {
        GateConfig::default()
    } else if let Some(path) = config_path {
        GateConfig::load_from(path)
    } else {
        GateConfig::load()
    }
}

pub fn load_configs(config_path: Option<&PathBuf>, use_defaults: bool) -> (Config, Config) {
    let defaults = || (Config::python_defaults(), Config::rust_defaults());
    if use_defaults {
        return defaults();
    }
    let Some(path) = config_path else {
        return (
            Config::load_for_language(ConfigLanguage::Python),
            Config::load_for_language(ConfigLanguage::Rust),
        );
    };
    let Ok(content) = std::fs::read_to_string(path) else {
        eprintln!("Warning: Config file not found: {}", path.display());
        return defaults();
    };
    if let Err(e) = content.parse::<toml::Table>() {
        eprintln!("Warning: Failed to parse config {}: {}", path.display(), e);
        return defaults();
    }
    (
        Config::load_from_content(&content, ConfigLanguage::Python),
        Config::load_from_content(&content, ConfigLanguage::Rust),
    )
}

pub fn config_provenance() -> String {
    let local = Path::new(".kissconfig");
    let home = std::env::var_os("HOME")
        .map(|h| Path::new(&h).join(".kissconfig"))
        .filter(|p| p.exists());
    let local_status = if local.exists() { "found" } else { "not found" };
    let home_status = home.as_ref().map_or_else(
        || "not found".to_string(),
        |p| format!("found: {}", p.display()),
    );
    format!("Config: defaults + ~/.kissconfig ({home_status}) + ./.kissconfig ({local_status})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_init_command_nonexistent_path() {
        let result = run_init_command(Path::new("/nonexistent/path/xyz"));
        assert_eq!(result, 1);
    }

    #[test]
    fn test_run_init_command_file_not_dir() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let result = run_init_command(tmp.path());
        assert_eq!(result, 1);
    }

    #[test]
    fn test_run_init_command_existing_config() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".kissconfig"), "# existing").unwrap();
        let result = run_init_command(tmp.path());
        assert_eq!(result, 0);
    }
}
