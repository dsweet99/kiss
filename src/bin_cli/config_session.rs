use crate::bin_cli::args::Commands;
use crate::bin_cli::mimic::run_mimic;
use crate::bin_cli::util::default_check_ignore_prefixes;
use kiss::{parse_target_arg, Config, ConfigLanguage, GateConfig};
use std::path::{Path, PathBuf};

pub fn command_paths(command: &Commands) -> Vec<String> {
    match command {
        Commands::Check { paths, .. }
        | Commands::Stats { paths, .. }
        | Commands::Mimic { paths, .. }
        | Commands::Viz { paths, .. }
        | Commands::Mv { paths, .. } => paths.clone(),
        Commands::Shrink { paths, target, .. } => {
            let mut out = paths.clone();
            if let Some(t) = target
                && parse_target_arg(t).is_err()
            {
                out.insert(0, t.clone());
            }
            out
        }
        Commands::Dry { path, .. } => vec![path.clone()],
        Commands::Clamp { .. } | Commands::Rules | Commands::Config | Commands::Test { .. } => {
            vec![".".to_string()]
        }
        Commands::Init { .. } => Vec::new(),
    }
}

fn config_path_for_paths(paths: &[String]) -> PathBuf {
    let first = paths.first().map(String::as_str).unwrap_or(".");
    let anchor = Path::new(first);
    if anchor.is_file() {
        anchor
            .parent()
            .unwrap_or(Path::new("."))
            .join(".kissconfig")
    } else {
        anchor.join(".kissconfig")
    }
}

pub fn ensure_default_config_exists(paths: &[String], use_defaults: bool) {
    if use_defaults || paths.is_empty() {
        return;
    }
    let local_config = config_path_for_paths(paths);
    if local_config.exists() {
        return;
    }
    let ignore = default_check_ignore_prefixes();
    let (py_files, rs_files) = kiss::discovery::gather_files_by_lang(paths, None, &ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        return;
    }
    run_mimic(paths, Some(&local_config), None, &ignore);
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
) -> kiss::TestSectionConfig {
    if use_defaults {
        kiss::TestSectionConfig::default()
    } else if let Some(path) = config_path {
        kiss::TestSectionConfig::load_from(path)
    } else {
        kiss::TestSectionConfig::load()
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
    fn test_ensure_default_config_exists_runs_clamp() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("sample.py"), "def foo():\n    return 1\n").unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        assert!(!Path::new(".kissconfig").exists());
        ensure_default_config_exists(&[".".to_string()], false);
        assert!(
            Path::new(".kissconfig").exists(),
            "missing local .kissconfig should be created by clamp"
        );

        std::env::set_current_dir(orig_dir).unwrap();
    }

    #[test]
    fn test_ensure_default_config_skipped_with_defaults_flag() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("sample.py"), "def foo():\n    return 1\n").unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        ensure_default_config_exists(&[".".to_string()], true);
        assert!(
            !Path::new(".kissconfig").exists(),
            "--defaults must not write .kissconfig"
        );

        std::env::set_current_dir(orig_dir).unwrap();
    }
}
