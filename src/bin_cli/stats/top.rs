use crate::bin_cli::config_session::config_provenance;
use kiss::Language;
use std::path::PathBuf;

pub fn run_stats_top(paths: &[String], lang_filter: Option<Language>, ignore: &[String], n: usize) {
    let (py_files, rs_files) = kiss::discovery::gather_files_by_lang(paths, lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        eprintln!("No source files found.");
        std::process::exit(1);
    }
    println!(
        "kiss stats --all {n} - Top Outliers\nAnalyzed from: {}\n{}\n",
        paths.join(", "),
        config_provenance()
    );
    let all_units = collect_all_units(&py_files, &rs_files);
    print_all_top_metrics(&all_units, n);
}

pub fn collect_all_units(py_files: &[PathBuf], rs_files: &[PathBuf]) -> Vec<kiss::UnitMetrics> {
    let mut all_units = Vec::new();
    all_units.extend(collect_py_units(py_files));
    all_units.extend(collect_rs_units(rs_files));
    all_units
}

fn collect_py_units(py_files: &[PathBuf]) -> Vec<kiss::UnitMetrics> {
    use kiss::parsing::parse_files;
    use kiss::{build_dependency_graph, collect_detailed_py};

    if py_files.is_empty() {
        return Vec::new();
    }
    match parse_files(py_files) {
        Ok(results) => {
            let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
            let graph = build_dependency_graph(&parsed);
            collect_detailed_py(&parsed, Some(&graph))
        }
        Err(e) => {
            eprintln!("error: failed to parse Python files: {e}");
            Vec::new()
        }
    }
}

fn collect_rs_units(rs_files: &[PathBuf]) -> Vec<kiss::UnitMetrics> {
    use kiss::rust_parsing::parse_rust_files;
    use kiss::rust_graph::build_rust_dependency_graph;
    use kiss::collect_detailed_rs;

    if rs_files.is_empty() {
        return Vec::new();
    }
    let results = parse_rust_files(rs_files);
    let parsed: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    let graph = build_rust_dependency_graph(&parsed);
    collect_detailed_rs(&parsed, Some(&graph))
}

type UnitMetricExtractor = fn(&kiss::UnitMetrics) -> Option<usize>;
type MetricSpec = (&'static str, UnitMetricExtractor);

pub fn print_all_top_metrics(units: &[kiss::UnitMetrics], n: usize) {
    let metrics: &[MetricSpec] = &[
        ("statements_per_function", |u| u.statements),
        ("args_total", |u| u.arguments),
        ("args_positional", |u| u.args_positional),
        ("args_keyword_only", |u| u.args_keyword_only),
        ("max_indentation_depth", |u| u.indentation),
        ("nested_function_depth", |u| u.nested_depth),
        ("branches_per_function", |u| u.branches),
        ("returns_per_function", |u| u.returns),
        ("return_values_per_function", |u| u.return_values),
        ("local_variables_per_function", |u| u.locals),
        ("methods_per_class", |u| u.methods),
        ("lines_per_file", |u| u.lines),
        ("imported_names_per_file", |u| u.imports),
        ("fan_in", |u| u.fan_in),
        ("fan_out", |u| u.fan_out),
        ("indirect_deps", |u| u.indirect_deps),
        ("dependency_depth", |u| u.dependency_depth),
    ];

    for (metric_id, extractor) in metrics {
        print_top_for_metric(units, n, metric_id, *extractor);
    }
}

pub fn print_top_for_metric<F>(units: &[kiss::UnitMetrics], n: usize, metric_id: &str, extractor: F)
where
    F: Fn(&kiss::UnitMetrics) -> Option<usize>,
{
    let mut with_values: Vec<_> = units
        .iter()
        .filter_map(|u| extractor(u).map(|v| (v, u)))
        .collect();
    if with_values.is_empty() {
        return;
    }
    with_values.sort_by(|a, b| b.0.cmp(&a.0));
    for (val, u) in with_values.into_iter().take(n) {
        println!(
            "STAT:{metric_id}:{val}:{file}:{line}:{name}",
            file = u.file,
            line = u.line,
            name = u.name
        );
    }
}
