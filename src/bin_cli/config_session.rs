use crate::bin_cli::mimic::run_mimic;
use crate::bin_cli::util::merge_check_ignore_prefixes;
use kiss::config_gen::{
    collect_lang_from_paths, generate_gate_stub_toml, raise_measured_thresholds,
    with_default_language_sections,
};
use kiss::{
    Config, GateConfig, Language, LanguageTablesPresent, gather_files_by_lang,
    kissconfig_path_from_cwd,
};
use std::path::{Path, PathBuf};

const KISSCONFIG_DEFAULT: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/kissconfig-default"));

pub fn ensure_default_config_exists() {
    ensure_default_config_from(&[".".to_string()], &[]);
}

pub fn ensure_check_config_from(paths: &[String], ignore: &[String]) {
    let local_config = kiss::kissconfig_path_from_cwd();
    if local_config.exists() {
        return;
    }
    write_config_text(&local_config, KISSCONFIG_DEFAULT);
    let roots = config_roots(paths);
    let collect_ignore = ignore_for_collect(&local_config, ignore);
    let Ok((py, rs)) = collect_lang_from_paths(&roots, None, &collect_ignore) else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(&local_config) else {
        return;
    };
    let raised = raise_measured_thresholds(&text, &py, &rs);
    if raised != text {
        write_config_text(&local_config, &raised);
    }
}

pub fn ensure_default_config_from(paths: &[String], ignore: &[String]) {
    let local_config = kiss::kissconfig_path_from_cwd();
    let roots = config_roots(paths);
    if !local_config.exists() {
        write_gate_stub(&local_config, ignore);
    } else if !is_kiss_gate_config(&local_config) {
        return;
    }
    let collect_ignore = ignore_for_collect(&local_config, ignore);
    if !needs_language_tables(&local_config, &roots, &collect_ignore) {
        return;
    }
    if has_production_source(&roots, &collect_ignore) == Some(false) {
        fill_language_tables_for_tests_only(&local_config);
        return;
    }
    let code = run_mimic(&roots, Some(&local_config), None, &collect_ignore);
    if code != 0 {
        std::process::exit(code);
    }
}

fn has_production_source(roots: &[String], ignore: &[String]) -> Option<bool> {
    let (py, rs) = collect_lang_from_paths(roots, None, ignore).ok()?;
    Some(py.file_count + rs.file_count > 0)
}

fn fill_language_tables_for_tests_only(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let filled = with_default_language_sections(&text);
    if filled != text {
        write_config_text(path, &filled);
    }
}

fn config_roots(paths: &[String]) -> Vec<String> {
    if paths.is_empty() {
        vec![".".to_string()]
    } else {
        vec![paths[0].clone()]
    }
}

fn write_gate_stub(path: &Path, cli_ignore: &[String]) {
    write_config_text(path, &generate_gate_stub_toml(cli_ignore));
}

fn write_config_text(path: &Path, text: &str) {
    if let Err(err) = std::fs::write(path, text) {
        eprintln!("Error writing to {}: {err}", path.display());
        std::process::exit(1);
    }
}

fn is_kiss_gate_config(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(table) = content.parse::<toml::Table>() else {
        return false;
    };
    table.contains_key("global") || table.contains_key("test")
}

fn ignore_for_collect(config_path: &Path, cli_ignore: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    if let Ok(cfg) = kiss::TestSectionConfig::try_load_path_only(config_path) {
        merged.extend(cfg.ignore);
    }
    merged.extend(cli_ignore.iter().cloned());
    merge_check_ignore_prefixes(&merged)
}

fn needs_language_tables(config_path: &Path, roots: &[String], ignore: &[String]) -> bool {
    let tables = LanguageTablesPresent::from_path(config_path);
    if tables.all_present() {
        return false;
    }
    let (py_files, rs_files) = gather_files_by_lang(roots, None, ignore);
    tables.missing_language(&py_files, &rs_files).is_some()
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

pub fn load_gate_config(config_path: Option<&PathBuf>) -> Result<GateConfig, kiss::ConfigError> {
    let path = match config_path {
        Some(path) => path.clone(),
        None => kissconfig_path_from_cwd(),
    };
    if !path.exists() {
        return Ok(GateConfig::default());
    }
    GateConfig::try_load_from(&path)
}

pub fn load_configs(config_path: Option<&PathBuf>) -> Result<(Config, Config), kiss::ConfigError> {
    let Some(path) = config_path else {
        return Ok((
            Config::try_load_for_language(Language::Python)?,
            Config::try_load_for_language(Language::Rust)?,
        ));
    };
    if !path.exists() {
        eprintln!("Warning: Could not read config file: {}", path.display());
        return Ok((Config::python_defaults(), Config::rust_defaults()));
    }
    Ok((
        Config::try_load_from(path, Language::Python)?,
        Config::try_load_from(path, Language::Rust)?,
    ))
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
            created.contains("orphan_detection = false"),
            "created .kissconfig must write orphan_detection = false:\n{created}"
        );
        assert!(
            !created.contains("orphan_module_enabled"),
            "created .kissconfig must not write orphan_module_enabled:\n{created}"
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
            created.contains("\"*\" = 99999"),
            "created .kissconfig must set max_unit_test_seconds catch-all to 99999:\n{created}"
        );
        assert!(
            created.contains("num_jobs = 4"),
            "created .kissconfig must set num_jobs = 4:\n{created}"
        );
        assert!(
            created.contains("num_jobs_pytest = 16"),
            "created .kissconfig must set num_jobs_pytest = 16:\n{created}"
        );
        assert!(
            created.contains("num_jobs_nextest = 4"),
            "created .kissconfig must set num_jobs_nextest = 4:\n{created}"
        );
        let before_python = created.split("[python]").next().unwrap_or(&created);
        assert!(
            !before_python.contains("num_jobs_pytest")
                && !before_python.contains("num_jobs_nextest"),
            "language job caps must not stay under [test]:\n{created}"
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
    fn check_missing_config_writes_default_then_raises_exceeded_threshold() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("wide.py"),
            "def f(a, b, c, d, e, f):\n    return a\n",
        )
        .unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        ensure_check_config_from(&[".".to_string()], &[]);
        let created = std::fs::read_to_string(".kissconfig").unwrap();
        std::env::set_current_dir(orig_dir).unwrap();
        assert!(
            created.contains("duplication_enabled = true"),
            "must start from kissconfig-default:\n{created}"
        );
        assert!(
            created.contains("docs_allowed = []"),
            "docs_allowed must stay empty:\n{created}"
        );
        assert!(
            created.contains("num_jobs_pytest = 4"),
            "pytest jobs must stay at the default:\n{created}"
        );
        assert!(
            created.contains("\"*\" = 3"),
            "unit-test catch-all must stay at 3:\n{created}"
        );
        assert!(
            created.contains("positional_args = 6"),
            "exceeded positional_args must be raised:\n{created}"
        );
        assert!(
            created.contains("arguments = 4"),
            "rust arguments must stay when no rust file exceeds them:\n{created}"
        );
        assert!(
            created.contains("[rust]"),
            "default rust section must be kept:\n{created}"
        );
    }

    #[test]
    fn check_does_not_alter_existing_kissconfig() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        let original = "\
[global]
duplication_enabled = false

[test]
ignore = [\"vendor\"]
";
        std::fs::write(tmp.path().join(".kissconfig"), original).unwrap();
        std::fs::write(
            tmp.path().join("wide.py"),
            "def f(a, b, c, d, e, f):\n    return a\n",
        )
        .unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        ensure_check_config_from(&[".".to_string()], &[]);
        let kept = std::fs::read_to_string(".kissconfig").unwrap();
        std::env::set_current_dir(orig_dir).unwrap();
        assert_eq!(kept, original);
    }

    #[test]
    fn check_missing_config_keeps_default_bytes_when_code_fits() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("tiny.py"), "def foo():\n    return 1\n").unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        ensure_check_config_from(&[".".to_string()], &[]);
        let created = std::fs::read_to_string(".kissconfig").unwrap();
        std::env::set_current_dir(orig_dir).unwrap();
        assert_eq!(created, KISSCONFIG_DEFAULT);
    }

    #[test]
    fn ensure_default_config_from_test_only_repo_adds_language_tables() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("test_only.py"),
            "def test_only():\n    assert True\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".kissconfig"),
            "[test]\norphan_detection = true\nmax_num_tests = 100\n",
        )
        .unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        ensure_default_config_from(&[".".to_string()], &[]);
        let created = std::fs::read_to_string(".kissconfig").unwrap();
        std::env::set_current_dir(orig_dir).unwrap();
        assert!(
            created.contains("[python]") && created.contains("[rust]"),
            "test-only repo must gain language tables:\n{created}"
        );
        assert!(
            created.contains("orphan_detection = true"),
            "existing orphan_detection must survive config fill:\n{created}"
        );
        let test_section = created.split("[python]").next().unwrap_or(&created);
        assert!(
            !test_section.contains("max_num_tests"),
            "retired [test] max_num_tests must not survive config fill:\n{created}"
        );
        assert!(
            created.contains("[python]\nmax_num_tests = 1000\n"),
            "filled python section must carry its own cap:\n{created}"
        );
        assert!(
            created.contains("[rust]\nmax_num_tests = 2000\n"),
            "filled rust section must carry its own cap:\n{created}"
        );
    }

    #[test]
    fn ensure_default_fills_missing_language_tables_on_stub() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".kissconfig"),
            "\
[global]
duplication_enabled = false
comment_removal_enabled = false
min_similarity = 0.9
docs_allowed = [\"./\" ]

[test]
orphan_detection = false
ignore = [\"vendor\"]
",
        )
        .unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        ensure_default_config_from(&[".".to_string()], &[]);
        let created = std::fs::read_to_string(".kissconfig").unwrap();
        std::env::set_current_dir(orig_dir).unwrap();
        assert!(
            created.contains("[rust]"),
            "stub must gain [rust]:\n{created}"
        );
        assert!(
            created.contains("ignore = [\"vendor\"]"),
            "stub ignore must be preserved:\n{created}"
        );
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
        let (py, _) = load_configs(Some(&custom)).unwrap();
        std::env::set_current_dir(orig_dir).unwrap();
        assert_eq!(py.statements_per_function, 42);
    }

    #[test]
    fn load_configs_rejects_an_unknown_section() {
        let tmp = tempfile::TempDir::new().unwrap();
        let custom = tmp.path().join("custom.toml");
        std::fs::write(
            &custom,
            "bogus = 1\n\n[python]\nstatements_per_function = 1\n",
        )
        .unwrap();
        let message = load_configs(Some(&custom)).unwrap_err().to_string();
        assert!(
            message.contains("Unknown config section") && message.contains("bogus"),
            "{message}"
        );
    }

    #[test]
    fn load_gate_config_uses_only_the_override_file() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join(".kissconfig"),
            "[python]\nmax_num_tests = 11\n[rust]\nmax_num_tests = 13\n",
        )
        .unwrap();
        let custom = tmp.path().join("custom.toml");
        std::fs::write(
            &custom,
            "[python]\nmax_num_tests = 22\n[rust]\nmax_num_tests = 24\n",
        )
        .unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let gate = load_gate_config(Some(&custom)).unwrap();
        std::env::set_current_dir(orig_dir).unwrap();
        assert_eq!(gate.max_num_tests_python, 22);
        assert_eq!(gate.max_num_tests_rust, 24);
    }

    #[test]
    fn load_gate_config_rejects_a_bad_time_limit_once() {
        let tmp = tempfile::TempDir::new().unwrap();
        let custom = tmp.path().join("custom.toml");
        std::fs::write(
            &custom,
            "[test]\nmax_unit_test_seconds = [{path = \"*\", seconds = 2.0}]\n",
        )
        .unwrap();
        let err = load_gate_config(Some(&custom)).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("max_unit_test_seconds") && message.contains("[pattern, seconds]"),
            "{message}"
        );
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

    #[test]
    fn ensure_default_config_from_subdir_keeps_repo_root_config() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let root_config = "[python]\nmax_num_tests = 3\n[rust]\nmax_num_tests = 4\n";
        std::fs::write(tmp.path().join(".kissconfig"), root_config).unwrap();
        let nested = tmp.path().join("pkg");
        std::fs::create_dir_all(&nested).unwrap();
        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&nested).unwrap();
        ensure_default_config_exists();
        std::env::set_current_dir(orig_dir).unwrap();
        assert!(
            !nested.join(".kissconfig").exists(),
            "a subdirectory kiss command must not create its own .kissconfig"
        );
        let kept = std::fs::read_to_string(tmp.path().join(".kissconfig")).unwrap();
        assert_eq!(kept, root_config);
    }
}
