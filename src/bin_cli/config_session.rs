use crate::bin_cli::mimic::run_mimic;
use crate::bin_cli::util::merge_check_ignore_prefixes;
use kiss::{Config, ConfigLanguage, GateConfig, LanguageTablesPresent, kissconfig_path_from_cwd};
use std::path::{Path, PathBuf};

pub fn ensure_default_config_exists() {
    ensure_default_config_from(&[".".to_string()], &[]);
}

pub fn ensure_default_config_from(paths: &[String], ignore: &[String]) {
    let local_config = Path::new(".kissconfig");
    if local_config.exists() {
        return;
    }
    let ignore = merge_check_ignore_prefixes(ignore);
    let roots = if paths.is_empty() {
        vec![".".to_string()]
    } else {
        vec![paths[0].clone()]
    };
    let code = run_mimic(&roots, Some(local_config), None, &ignore);
    if code != 0 {
        std::process::exit(code);
    }
}

pub fn load_language_tables(config_path: Option<&PathBuf>) -> LanguageTablesPresent {
    let path = match config_path {
        Some(path) => path.clone(),
        None => kissconfig_path_from_cwd(),
    };
    LanguageTablesPresent::from_path_or_both(&path)
}

pub fn load_test_section_config(
    config_path: Option<&PathBuf>,
) -> Result<kiss::TestSectionConfig, kiss::ConfigError> {
    if let Some(path) = config_path {
        kiss::TestSectionConfig::try_load_path_only(path)
    } else {
        kiss::TestSectionConfig::try_load()
    }
}

pub fn load_gate_config(config_path: Option<&PathBuf>) -> GateConfig {
    if let Some(path) = config_path {
        GateConfig::load_from(path)
    } else {
        GateConfig::load()
    }
}

pub fn load_configs(config_path: Option<&PathBuf>) -> (Config, Config) {
    let Some(path) = config_path else {
        return (
            Config::load_for_language(ConfigLanguage::Python),
            Config::load_for_language(ConfigLanguage::Rust),
        );
    };
    (
        Config::load_from_for_language(path, ConfigLanguage::Python),
        Config::load_from_for_language(path, ConfigLanguage::Rust),
    )
}

pub fn config_provenance(config: Option<&Path>) -> String {
    match config {
        Some(path) => {
            let status = if path.exists() { "found" } else { "not found" };
            format!("Config: defaults + {} ({status})", path.display())
        }
        None => {
            let local = Path::new(".kissconfig");
            let local_status = if local.exists() { "found" } else { "not found" };
            format!("Config: defaults + ./.kissconfig ({local_status})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bin_cli::args::Cli;
    use clap::Parser;

    fn init_parse_is_err(args: &[&str]) {
        assert!(Cli::try_parse_from(args).is_err());
    }

    #[test]
    fn test_run_init_command_nonexistent_path() {
        init_parse_is_err(&["kiss", "init", "/nonexistent/path/xyz"]);
    }

    #[test]
    fn test_run_init_command_file_not_dir() {
        init_parse_is_err(&["kiss", "init", "/etc/hosts"]);
    }

    #[test]
    fn test_run_init_command_existing_config() {
        init_parse_is_err(&["kiss", "init"]);
    }

    #[test]
    fn test_run_init_command_writes_test_section_defaults() {
        init_parse_is_err(&["kiss", "init"]);
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
            "missing local .kissconfig should be created from codebase maxima"
        );
        let created = std::fs::read_to_string(".kissconfig").unwrap();
        assert!(
            created.contains("[test]"),
            "created .kissconfig must include [test]:\n{created}"
        );
        assert!(
            created.contains("duplication_enabled = false"),
            "created .kissconfig must disable duplication:\n{created}"
        );
        assert!(
            created.contains("orphan_module_enabled = false"),
            "created .kissconfig must disable orphan_module:\n{created}"
        );
        assert!(
            created.contains("comment_removal_enabled = false"),
            "created .kissconfig must disable comment_removal:\n{created}"
        );
        assert!(
            created.contains(r#"docs_allowed = ["./"]"#),
            r#"created .kissconfig must set docs_allowed = ["./"]:
{created}"#,
        );
        assert!(
            created.contains("test_coverage_threshold = 0"),
            "created .kissconfig must set test_coverage_threshold = 0:\n{created}"
        );
        assert!(
            created.contains("\"*\" = 99999"),
            "created .kissconfig must set max_unit_test_seconds catch-all to 99999:\n{created}"
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
    fn test_ensure_default_config_from_uses_given_root() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let cwd = tempfile::TempDir::new().unwrap();
        let other = tempfile::TempDir::new().unwrap();
        std::fs::write(cwd.path().join("tiny.py"), "def tiny():\n    return 1\n").unwrap();
        std::fs::write(
            other.path().join("mod.py"),
            "def f(a, b, c, d, e, f):\n    return a\n",
        )
        .unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(cwd.path()).unwrap();
        ensure_default_config_from(&[other.path().to_string_lossy().into_owned()], &[]);
        let created = std::fs::read_to_string(".kissconfig").unwrap();
        std::env::set_current_dir(orig_dir).unwrap();
        assert!(
            created.contains("positional_args = 6"),
            "config must use the given root, not cwd:\n{created}"
        );
    }

    #[test]
    fn test_load_test_section_config_defaults() {
        let missing = PathBuf::from("/nonexistent/kiss-override.toml");
        let cfg = load_test_section_config(Some(&missing)).unwrap();

        assert_eq!(
            cfg.main_branch,
            kiss::TestSectionConfig::default().main_branch
        );
    }

    #[test]
    fn load_configs_uses_only_the_override_file() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join(".kissconfig"),
            "[python]\nstatements_per_function = 100\n",
        )
        .unwrap();
        let custom = tmp.path().join("custom.toml");
        std::fs::write(&custom, "[python]\nstatements_per_function = 42\n").unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let (py, _) = load_configs(Some(&custom));
        std::env::set_current_dir(orig_dir).unwrap();
        assert_eq!(py.statements_per_function, 42);
    }

    #[test]
    fn load_gate_config_uses_only_the_override_file() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join(".kissconfig"),
            "[test]\ntest_coverage_threshold = 11\n",
        )
        .unwrap();
        let custom = tmp.path().join("custom.toml");
        std::fs::write(&custom, "[test]\ntest_coverage_threshold = 22\n").unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let gate = load_gate_config(Some(&custom));
        std::env::set_current_dir(orig_dir).unwrap();
        assert_eq!(gate.test_coverage_threshold, 22);
    }

    #[test]
    fn config_provenance_names_override_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let custom = tmp.path().join("custom.toml");
        std::fs::write(&custom, "[python]\n").unwrap();
        let text = config_provenance(Some(&custom));
        assert!(
            text.contains("custom.toml") && text.contains("found"),
            "{text}"
        );
        assert!(!text.contains("./.kissconfig"), "{text}");
        let missing = tmp.path().join("absent.toml");
        let missing_text = config_provenance(Some(&missing));
        assert!(
            missing_text.contains("absent.toml") && missing_text.contains("not found"),
            "{missing_text}"
        );
    }
}
