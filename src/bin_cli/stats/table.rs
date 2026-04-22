use crate::bin_cli::config_session::config_provenance;
use kiss::Language;

pub fn run_stats_table(paths: &[String], lang_filter: Option<Language>, ignore: &[String]) {
    let (py_files, rs_files) = kiss::discovery::gather_files_by_lang(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        eprintln!("No source files found.");
        std::process::exit(1);
    }
    println!(
        "kiss stats --table - Per-Unit Metrics\nAnalyzed from: {}\n{}\n",
        paths.join(", "),
        config_provenance()
    );
    print_py_table(&py_files);
    print_rs_table(&rs_files);
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
