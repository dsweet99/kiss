use kiss::Language;
use kiss::config_gen::{
    GenerateConfigParams, collect_all_stats_with_ignore, generate_config_toml_by_language,
    infer_gate_config_for_paths, write_mimic_config,
};
use std::path::Path;

fn mimic_toml(
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
) -> Result<(String, usize, usize), String> {
    let ((py_stats, py_cnt), (rs_stats, rs_cnt)) =
        collect_all_stats_with_ignore(paths, lang_filter, ignore);
    if py_cnt + rs_cnt == 0 {
        return Err("No source files found.".to_string());
    }
    let gate = infer_gate_config_for_paths(paths, lang_filter, ignore);
    let toml = generate_config_toml_by_language(&GenerateConfigParams {
        py: &py_stats,
        rs: &rs_stats,
        py_n: py_cnt,
        rs_n: rs_cnt,
        gate: &gate,
    });
    Ok((toml, py_cnt, rs_cnt))
}

fn write_mimic_output(out: &Path, toml: &str, py_cnt: usize, rs_cnt: usize) -> Result<(), String> {
    write_mimic_config(out, toml, py_cnt, rs_cnt)
        .map_err(|e| format!("Error writing to {}: {e}", out.display()))
}

fn run_mimic_result(
    paths: &[String],
    out: Option<&Path>,
    lang_filter: Option<Language>,
    ignore: &[String],
) -> Result<Option<String>, String> {
    let (toml, py_cnt, rs_cnt) = mimic_toml(paths, lang_filter, ignore)?;
    match out {
        Some(p) => {
            write_mimic_output(p, &toml, py_cnt, rs_cnt)?;
            Ok(None)
        }
        None => Ok(Some(toml)),
    }
}

pub fn run_mimic(
    paths: &[String],
    out: Option<&Path>,
    lang_filter: Option<Language>,
    ignore: &[String],
) {
    match run_mimic_result(paths, out, lang_filter, ignore) {
        Ok(Some(toml)) => print!("{toml}"),
        Ok(None) => {}
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mimic_toml_rejects_empty_inputs_without_exiting() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = vec![tmp.path().to_string_lossy().to_string()];

        let err = mimic_toml(&paths, Some(Language::Python), &[]).unwrap_err();

        assert_eq!(err, "No source files found.");
    }

    #[test]
    fn write_mimic_output_reports_unwritable_destination() {
        let tmp = tempfile::TempDir::new().unwrap();
        let out = tmp.path().join("missing").join("config.toml");

        let err =
            write_mimic_output(&out, "[gate]\ntest_coverage_threshold = 100\n", 1, 0).unwrap_err();

        assert!(err.starts_with("Error writing to "));
        assert!(err.contains("config.toml"));
    }

    #[test]
    fn run_mimic_result_returns_toml_for_stdout_mode() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("app.py"), "def f():\n    return 1\n").unwrap();
        let paths = vec![tmp.path().to_string_lossy().to_string()];

        let toml = run_mimic_result(&paths, None, Some(Language::Python), &[])
            .unwrap()
            .unwrap();

        assert!(toml.contains("[python]"));
        assert!(toml.contains("[gate]"));
    }

    #[test]
    fn run_mimic_result_writes_file_for_output_mode() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("app.py"), "def f():\n    return 1\n").unwrap();
        let paths = vec![tmp.path().to_string_lossy().to_string()];
        let out = tmp.path().join("kiss.toml");

        let result = run_mimic_result(&paths, Some(&out), Some(Language::Python), &[]).unwrap();

        assert_eq!(result, None);
        let written = std::fs::read_to_string(out).unwrap();
        assert!(written.contains("[python]"));
        assert!(written.contains("[gate]"));
    }

    #[test]
    fn run_mimic_result_propagates_empty_input_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = vec![tmp.path().to_string_lossy().to_string()];

        let err =
            run_mimic_result(&paths, Some(tmp.path()), Some(Language::Python), &[]).unwrap_err();

        assert_eq!(err, "No source files found.");
    }

    #[test]
    fn run_mimic_result_propagates_write_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("app.py"), "def f():\n    return 1\n").unwrap();
        let paths = vec![tmp.path().to_string_lossy().to_string()];
        let out = tmp.path().join("missing").join("kiss.toml");

        let err = run_mimic_result(&paths, Some(&out), Some(Language::Python), &[]).unwrap_err();

        assert!(err.starts_with("Error writing to "));
        assert!(err.contains("kiss.toml"));
    }
}
