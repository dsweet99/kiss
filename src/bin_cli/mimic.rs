use kiss::Language;
use kiss::config_gen::{
    GenerateConfigParams, collect_lang_from_paths, generate_config_toml_by_language,
    infer_gate_config_for_paths, write_mimic_config_with_quiet,
};
use std::fmt::Display;
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
    match mimic_generate(paths, out, lang_filter, ignore, quiet) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

fn mimic_generate(
    paths: &[String],
    out: Option<&Path>,
    lang_filter: Option<Language>,
    ignore: &[String],
    quiet: bool,
) -> Result<(), i32> {
    let (py, rs) = collect_lang_from_paths(paths, lang_filter, ignore)
        .map_err(|err| mimic_fail(quiet, err))?;
    if py.file_count + rs.file_count == 0 {
        return Err(mimic_fail(quiet, "No source files found."));
    }
    let gate = infer_gate_config_for_paths(paths, lang_filter, ignore)
        .map_err(|err| mimic_fail(quiet, err))?;
    let toml = generate_config_toml_by_language(&GenerateConfigParams {
        py: &py.stats,
        rs: &rs.stats,
        py_n: py.file_count,
        rs_n: rs.file_count,
        py_graph: py.graph_max,
        rs_graph: rs.graph_max,
        gate: &gate,
    });
    write_mimic_toml(out, &toml, py.file_count, rs.file_count, quiet)
}

fn write_mimic_toml(
    out: Option<&Path>,
    toml: &str,
    py_n: usize,
    rs_n: usize,
    quiet: bool,
) -> Result<(), i32> {
    let Some(path) = out else {
        if !quiet {
            print!("{toml}");
        }
        return Ok(());
    };
    write_mimic_config_with_quiet(path, toml, py_n, rs_n, quiet)
        .map_err(|err| mimic_fail(quiet, format!("Error writing to {}: {err}", path.display())))
}

fn mimic_fail(quiet: bool, msg: impl Display) -> i32 {
    if !quiet {
        eprintln!("{msg}");
    }
    1
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
