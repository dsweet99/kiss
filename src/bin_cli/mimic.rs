use kiss::Language;
use kiss::config_gen::{
    GenerateConfigParams, collect_lang_from_paths, generate_config_toml_by_language,
    infer_gate_config_for_paths, write_mimic_config_with_quiet,
};
use std::path::Path;

pub fn run_mimic(
    paths: &[String],
    out: Option<&Path>,
    lang_filter: Option<Language>,
    ignore: &[String],
) -> i32 {
    run_mimic_with_quiet(paths, out, lang_filter, ignore, false)
}

pub fn run_mimic_with_quiet(
    paths: &[String],
    out: Option<&Path>,
    lang_filter: Option<Language>,
    ignore: &[String],
    quiet: bool,
) -> i32 {
    let (py, rs) = collect_lang_from_paths(paths, lang_filter, ignore);
    if py.file_count + rs.file_count == 0 {
        if !quiet {
            eprintln!("No source files found.");
        }
        return 1;
    }
    let gate = infer_gate_config_for_paths(paths, lang_filter, ignore);
    let toml = generate_config_toml_by_language(&GenerateConfigParams {
        py: &py.stats,
        rs: &rs.stats,
        py_n: py.file_count,
        rs_n: rs.file_count,
        py_graph: py.graph_max,
        rs_graph: rs.graph_max,
        gate: &gate,
    });
    match out {
        Some(p) => {
            if let Err(e) =
                write_mimic_config_with_quiet(p, &toml, py.file_count, rs.file_count, quiet)
            {
                if !quiet {
                    eprintln!("Error writing to {}: {e}", p.display());
                }
                return 1;
            }
        }
        None => {
            if !quiet {
                print!("{toml}");
            }
        }
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
