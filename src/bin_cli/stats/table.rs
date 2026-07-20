use crate::bin_cli::config_session::config_provenance;
use kiss::Language;

pub fn run_stats_table(paths: &[String], lang_filter: Option<Language>, ignore: &[String]) {
    let status = run_stats_table_status(paths, lang_filter, ignore);
    if status != 0 {
        std::process::exit(status);
    }
}

fn run_stats_table_status(
    paths: &[String],
    lang_filter: Option<Language>,
    ignore: &[String],
) -> i32 {
    let (py_files, rs_files) = kiss::discovery::gather_files_by_lang(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        return no_source_files_status();
    }
    println!(
        "kiss stats --table - Per-Unit Metrics\nAnalyzed from: {}\n{}\n",
        paths.join(", "),
        config_provenance()
    );
    print_py_table(&py_files);
    print_rs_table(&rs_files);
    0
}

fn no_source_files_status() -> i32 {
    eprintln!("No source files found.");
    1
}

fn print_py_table(py_files: &[std::path::PathBuf]) {
    use kiss::parsing::parse_files;
    use kiss::{build_dependency_graph, collect_detailed_py, format_detailed_table};

    if py_files.is_empty() {
        return;
    }
    match parse_files(py_files) {
        Ok(results) => {
            let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
            let graph = build_dependency_graph(&parsed);
            let units = collect_detailed_py(&parsed, Some(&graph));
            println!(
                "=== Python ({} files, {} units) ===\n{}",
                py_files.len(),
                units.len(),
                format_detailed_table(&units)
            );
        }
        Err(e) => eprintln!("error: failed to parse Python files: {e}"),
    }
}

fn print_rs_table(rs_files: &[std::path::PathBuf]) {
    use kiss::rust_graph::build_rust_dependency_graph;
    use kiss::rust_parsing::parse_rust_files;
    use kiss::{collect_detailed_rs, format_detailed_table};

    if rs_files.is_empty() {
        return;
    }
    let results = parse_rust_files(rs_files);
    let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    let graph = build_rust_dependency_graph(&parsed);
    let units = collect_detailed_rs(&parsed, Some(&graph));
    println!(
        "=== Rust ({} files, {} units) ===\n{}",
        rs_files.len(),
        units.len(),
        format_detailed_table(&units)
    );
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[test]
    fn private_table_printers_return_on_empty_inputs() {
        super::print_py_table(&[]);
        super::print_rs_table(&[]);
    }

    #[test]
    fn no_source_files_status_matches_cli_error_code() {
        assert_eq!(super::no_source_files_status(), 1);
    }

    #[test]
    fn run_stats_table_status_reports_empty_inputs_without_exiting() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let paths = vec![tmp.path().to_string_lossy().to_string()];

        assert_eq!(super::run_stats_table_status(&paths, None, &[]), 1);
    }

    #[test]
    fn private_table_printers_parse_tiny_python_and_rust_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let py_path = tmp.path().join("sample.py");
        let rs_path = tmp.path().join("sample.rs");
        fs::write(&py_path, "def f():\n    return 1\n").expect("write python");
        fs::write(&rs_path, "pub fn f() -> i32 {\n    1\n}\n").expect("write rust");

        super::print_py_table(&[py_path]);
        super::print_rs_table(&[rs_path]);
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

        super::run_stats_table(&paths, None, &[]);
    }
}
