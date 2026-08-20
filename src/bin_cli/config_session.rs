use crate::bin_cli::mimic::run_mimic_with_quiet;
use crate::bin_cli::util::merge_check_ignore_prefixes;
use kiss::{Config, ConfigLanguage, GateConfig, LanguageTablesPresent, kissconfig_path_from_cwd};
use std::path::{Path, PathBuf};

pub fn ensure_default_config_exists() {
    let local_config = Path::new(".kissconfig");
    if local_config.exists() {
        return;
    }
    let quiet = false;
    let ignore = merge_check_ignore_prefixes(&[]);
    let code = run_mimic_with_quiet(&[".".to_string()], Some(local_config), None, &ignore, quiet);
    if code != 0 {
        std::process::exit(code);
    }
}

pub fn load_language_tables(
    config_path: Option<&PathBuf>,
    use_defaults: bool,
) -> LanguageTablesPresent {
    if use_defaults {
        return LanguageTablesPresent::both();
    }
    if let Some(path) = config_path {
        return LanguageTablesPresent::from_path(path);
    }
    LanguageTablesPresent::from_path(&kissconfig_path_from_cwd())
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

pub fn load_test_section_config(
    config_path: Option<&PathBuf>,
    use_defaults: bool,
) -> Result<kiss::TestSectionConfig, kiss::ConfigError> {
    if use_defaults {
        Ok(kiss::TestSectionConfig::default())
    } else if let Some(path) = config_path {
        kiss::TestSectionConfig::try_load_from(path)
    } else {
        kiss::TestSectionConfig::try_load()
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
    (
        Config::load_for_language_with_override(path, ConfigLanguage::Python),
        Config::load_for_language_with_override(path, ConfigLanguage::Rust),
    )
}

pub fn config_provenance() -> String {
    let local = Path::new(".kissconfig");
    let local_status = if local.exists() { "found" } else { "not found" };
    format!("Config: defaults + ./.kissconfig ({local_status})")
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

    #[test]
    fn test_run_init_command_writes_test_section_defaults() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(run_init_command(tmp.path()), 0);
        let created = std::fs::read_to_string(tmp.path().join(".kissconfig")).unwrap();
        assert!(
            created.contains("num_jobs = 4")
                && created.contains("watch_settle_seconds = 1.0")
                && created.contains("pytest_plugins = []")
                && created.contains("ignore = []")
                && created.contains("max_num_tests = 999999")
                && created.contains("[test.max_unit_test_seconds]"),
            "kiss init must write [test] defaults:\n{created}"
        );
    }

    #[test]
    fn test_ensure_default_config_exists_runs_clamp() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("sample.py"), "def foo():\n    return 1\n").unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        assert!(!Path::new(".kissconfig").exists());
        ensure_default_config_exists();
        assert!(
            Path::new(".kissconfig").exists(),
            "missing local .kissconfig should be created by clamp"
        );
        let created = std::fs::read_to_string(".kissconfig").unwrap();
        assert!(
            created.contains("[test]"),
            "created .kissconfig must include [test]:\n{created}"
        );
        assert!(
            created.contains("num_jobs = 4"),
            "created .kissconfig must set num_jobs = 4:\n{created}"
        );
        assert!(
            created.contains("pytest_plugins = []"),
            "created .kissconfig must set pytest_plugins = []:\n{created}"
        );
        assert!(
            created.contains("ignore = []"),
            "created .kissconfig must set ignore = []:\n{created}"
        );

        std::env::set_current_dir(orig_dir).unwrap();
    }

    #[test]
    fn test_load_test_section_config_defaults() {
        let cfg = load_test_section_config(None, true).unwrap();

        assert_eq!(
            cfg.main_branch,
            kiss::TestSectionConfig::default().main_branch
        );
    }
}
