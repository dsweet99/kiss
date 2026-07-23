use crate::bin_cli::config_session::config_provenance;
use kiss::{Config, GateConfig, Language};
use std::path::PathBuf;

pub struct StatsTopArgs<'a> {
    pub paths: &'a [String],
    pub lang_filter: Option<Language>,
    pub ignore: &'a [String],
    pub n: usize,
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
    pub gate_config: &'a GateConfig,
}

pub fn run_stats_top(args: StatsTopArgs<'_>) {
    finalize_stats_top_status(run_stats_top_status(args));
}

pub(super) fn finalize_stats_top_status(status: i32) {
    if status == 0 {
        return;
    }
    std::process::exit(status);
}

pub(super) fn run_stats_top_status(args: StatsTopArgs<'_>) -> i32 {
    let _ = (args.py_config, args.rs_config, args.gate_config);
    let (py_files, rs_files) =
        kiss::discovery::gather_files_by_lang(args.paths, args.lang_filter, args.ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        return no_source_files_status();
    }
    println!(
        "kiss stats --all {n} - Top Outliers\nAnalyzed from: {paths}\n{prov}\n",
        n = args.n,
        paths = args.paths.join(", "),
        prov = config_provenance()
    );
    let py_units = collect_py_units(&py_files);
    let rs_units = collect_rs_units(&rs_files);
    let mut all_units = py_units;
    all_units.extend(rs_units);
    print_all_top_metrics(&all_units, args.n);
    0
}

fn no_source_files_status() -> i32 {
    eprintln!("No source files found.");
    1
}

#[cfg(test)]
pub(super) fn merge_fresh_items(
    _py: Option<()>,
    _rs: Option<()>,
) -> Option<()> {
    None
}

#[cfg(test)]
pub fn collect_all_units(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
) -> Vec<kiss::UnitMetrics> {
    let py_units = collect_py_units(py_files);
    let rs_units = collect_rs_units(rs_files);
    let mut units = py_units;
    units.extend(rs_units);
    units
}

fn collect_py_units(py_files: &[PathBuf]) -> Vec<kiss::UnitMetrics> {
    use kiss::parsing::parse_files;
    use kiss::{build_dependency_graph, collect_detailed_py};

    collect_lang_units(LangCollect {
        files: py_files,
        // parse_files currently always returns Ok(...); keep the Result surface explicit.
        parse: |files| {
            parse_files(files)
                .unwrap_or_else(|_| Vec::new())
                .into_iter()
                .filter_map(Result::ok)
                .collect()
        },
        build_graph: build_dependency_graph,
        collect_detailed: collect_detailed_py,
    })
}

fn collect_rs_units(rs_files: &[PathBuf]) -> Vec<kiss::UnitMetrics> {
    use kiss::rust_graph::build_rust_dependency_graph;
    use kiss::rust_parsing::parse_rust_files;
    use kiss::collect_detailed_rs;

    collect_lang_units(LangCollect {
        files: rs_files,
        parse: |files| {
            parse_rust_files(files)
                .into_iter()
                .filter_map(Result::ok)
                .collect()
        },
        build_graph: build_rust_dependency_graph,
        collect_detailed: collect_detailed_rs,
    })
}

struct LangCollect<'a, P, FParse, FBuild, FCollect>
where
    FParse: FnOnce(&[PathBuf]) -> Vec<P>,
    FBuild: FnOnce(&[&P]) -> kiss::DependencyGraph,
    FCollect: FnOnce(&[&P], Option<&kiss::DependencyGraph>) -> Vec<kiss::UnitMetrics>,
{
    files: &'a [PathBuf],
    parse: FParse,
    build_graph: FBuild,
    collect_detailed: FCollect,
}

fn collect_lang_units<P, FParse, FBuild, FCollect>(
    args: LangCollect<'_, P, FParse, FBuild, FCollect>,
) -> Vec<kiss::UnitMetrics>
where
    FParse: FnOnce(&[PathBuf]) -> Vec<P>,
    FBuild: FnOnce(&[&P]) -> kiss::DependencyGraph,
    FCollect: FnOnce(&[&P], Option<&kiss::DependencyGraph>) -> Vec<kiss::UnitMetrics>,
{
    if args.files.is_empty() {
        return Vec::new();
    }
    let parsed = (args.parse)(args.files);
    let parsed_refs: Vec<&P> = parsed.iter().collect();
    let graph = (args.build_graph)(&parsed_refs);
    let mut units = (args.collect_detailed)(&parsed_refs, Some(&graph));
    append_cycle_units(&mut units, &graph);
    units
}

pub(super) fn append_cycle_units(
    units: &mut Vec<kiss::UnitMetrics>,
    graph: &kiss::DependencyGraph,
) {
    for cycle in graph.find_cycles().cycles {
        let Some(representative) = cycle.iter().min().cloned() else {
            continue;
        };
        let path_str = graph
            .paths
            .get(&representative)
            .map_or_else(String::new, |p| p.display().to_string());
        let mut u = kiss::UnitMetrics::new(path_str, representative, "file", 1);
        u.cycle_size = Some(cycle.len());
        units.push(u);
    }
}

#[cfg(test)]
pub(super) const AGGREGATE_ONLY_METRICS: &[&str] = &[];

#[path = "top_extractors.rs"]
mod top_extractors;
pub(crate) use top_extractors::*;

pub fn print_all_top_metrics(units: &[kiss::UnitMetrics], n: usize) {
    for def in kiss::METRICS {
        if let Some(extractor) = extractor_for(def.metric_id) {
            print_top_for_metric(units, n, def.metric_id, extractor);
        }
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
