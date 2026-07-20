use kiss::Language;
use kiss::config_gen::{
    GenerateConfigParams, collect_all_stats_with_ignore, generate_config_toml_by_language,
    infer_gate_config_for_paths, write_mimic_config,
};
use std::path::Path;

pub fn run_mimic(
    paths: &[String],
    out: Option<&Path>,
    lang_filter: Option<Language>,
    ignore: &[String],
) -> i32 {
    let ((py_stats, py_cnt), (rs_stats, rs_cnt)) =
        collect_all_stats_with_ignore(paths, lang_filter, ignore);
    if py_cnt + rs_cnt == 0 {
        eprintln!("No source files found.");
        return 1;
    }
    let gate = infer_gate_config_for_paths(paths, lang_filter, ignore);
    let toml = generate_config_toml_by_language(&GenerateConfigParams {
        py: &py_stats,
        rs: &rs_stats,
        py_n: py_cnt,
        rs_n: rs_cnt,
        gate: &gate,
    });
    match out {
        Some(p) => {
            if let Err(e) = write_mimic_config(p, &toml, py_cnt, rs_cnt) {
                eprintln!("Error writing to {}: {e}", p.display());
                return 1;
            }
        }
        None => print!("{toml}"),
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mimic_reports_no_source_files_without_exiting() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = vec![tmp.path().to_string_lossy().to_string()];

        assert_eq!(run_mimic(&paths, None, None, &[]), 1);
    }

    #[test]
    fn mimic_reports_output_write_error_without_exiting() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("app.py"), "def value():\n    return 1\n").unwrap();
        let paths = vec![tmp.path().to_string_lossy().to_string()];

        assert_eq!(run_mimic(&paths, Some(tmp.path()), None, &[]), 1);
    }
}
