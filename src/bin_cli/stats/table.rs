use crate::bin_cli::config_session::config_provenance;
use kiss::Language;

#[cfg(test)]
pub fn run_stats_table(
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
    language_tables: kiss::LanguageTablesPresent,
) {
    let status = run_stats_table_status(paths, lang_filter, ignore, language_tables);
    if status != 0 {
        std::process::exit(status);
    }
}

pub(super) fn run_stats_table_status(
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
    language_tables: kiss::LanguageTablesPresent,
) -> i32 {
    let (py_files, rs_files) = kiss::discovery::gather_files_by_lang(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        return no_source_files_status();
    }
    if let Err(code) =
        crate::bin_cli::util::reject_unconfigured_languages(&py_files, &rs_files, language_tables)
    {
        return code;
    }
    println!(
        "kiss stats --table - Per-Unit Metrics\nAnalyzed from: {}\n{}\n",
        paths.join(", "),
        config_provenance()
    );
    if let Err(err) = print_py_table(&py_files).and_then(|()| print_rs_table(&rs_files)) {
        eprintln!("{err}");
        return 1;
    }
    0
}

fn no_source_files_status() -> i32 {
    eprintln!("No source files found.");
    1
}

fn print_py_table(py_files: &[std::path::PathBuf]) -> Result<(), kiss::RoleBuildError> {
    use kiss::{build_python_context_graph, collect_detailed_py, format_detailed_table};

    if py_files.is_empty() {
        return Ok(());
    }
    let (parsed, roles) = super::load::load_production_python(py_files)?;
    let refs: Vec<_> = parsed.iter().collect();
    let graph = build_python_context_graph(&refs, &roles).production_view();
    let units = collect_detailed_py(&refs, Some(&graph));
    println!(
        "=== Python ({} files, {} units) ===\n{}",
        parsed.len(),
        units.len(),
        format_detailed_table(&units)
    );
    Ok(())
}

fn print_rs_table(rs_files: &[std::path::PathBuf]) -> Result<(), kiss::RoleBuildError> {
    use kiss::{
        build_rust_dependency_graph_with_roles, collect_detailed_rs_with_roles,
        format_detailed_table,
    };

    if rs_files.is_empty() {
        return Ok(());
    }
    let (parsed, roles) = super::load::load_production_rust(rs_files)?;
    let refs: Vec<_> = parsed.iter().collect();
    let graph = build_rust_dependency_graph_with_roles(&refs, Some(&roles));
    let units = collect_detailed_rs_with_roles(&refs, Some(&graph), Some(&roles));
    println!(
        "=== Rust ({} files, {} units) ===\n{}",
        parsed.len(),
        units.len(),
        format_detailed_table(&units)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[test]
    fn private_table_printers_return_on_empty_inputs() {
        super::print_py_table(&[]).unwrap();
        super::print_rs_table(&[]).unwrap();
    }

    #[test]
    fn no_source_files_status_matches_cli_error_code() {
        assert_eq!(super::no_source_files_status(), 1);
    }

    #[test]
    fn run_stats_table_status_reports_empty_inputs_without_exiting() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let paths = vec![tmp.path().to_string_lossy().to_string()];

        assert_eq!(
            super::run_stats_table_status(&paths, None, &[], kiss::LanguageTablesPresent::both()),
            1
        );
    }

    #[test]
    fn private_table_printers_parse_tiny_python_and_rust_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let py_path = tmp.path().join("sample.py");
        let rs_path = tmp.path().join("sample.rs");
        fs::write(&py_path, "def f():\n    return 1\n").expect("write python");
        fs::write(&rs_path, "pub fn f() -> i32 {\n    1\n}\n").expect("write rust");

        super::print_py_table(&[py_path]).unwrap();
        super::print_rs_table(&[rs_path]).unwrap();
    }

    #[test]
    fn run_stats_table_handles_tiny_mixed_sources() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let py_path = tmp.path().join("sample.py");
        let rs_path = tmp.path().join("sample.rs");
        fs::write(&py_path, "def f():\n    return 1\n").expect("write python");
        fs::write(&rs_path, "pub fn f() -> i32 {\n    1\n}\n").expect("write rust");
        let paths = vec![
            py_path.to_string_lossy().to_string(),
            rs_path.to_string_lossy().to_string(),
        ];

        super::run_stats_table(&paths, None, &[], kiss::LanguageTablesPresent::both());
    }
}
