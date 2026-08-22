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
    pub language_tables: kiss::LanguageTablesPresent,
}

#[cfg(test)]
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
    if let Err(code) = crate::bin_cli::util::reject_unconfigured_languages(
        &py_files,
        &rs_files,
        args.language_tables,
    ) {
        return code;
    }
    println!(
        "kiss stats --all {n} - Top Outliers\nAnalyzed from: {paths}\n{prov}\n",
        n = args.n,
        paths = args.paths.join(", "),
        prov = config_provenance()
    );
    let all_units = match collect_py_units(&py_files).and_then(|py| {
        collect_rs_units(&rs_files).map(|rs| {
            let mut units = py;
            units.extend(rs);
            units
        })
    }) {
        Ok(units) => units,
        Err(err) => {
            eprintln!("{err}");
            return 1;
        }
    };
    print_all_top_metrics(&all_units, args.n);
    0
}

fn no_source_files_status() -> i32 {
    eprintln!("No source files found.");
    1
}

#[cfg(test)]
pub(super) fn merge_fresh_items(_py: Option<()>, _rs: Option<()>) -> Option<()> {
    None
}

#[cfg(test)]
pub fn collect_all_units(py_files: &[PathBuf], rs_files: &[PathBuf]) -> Vec<kiss::UnitMetrics> {
    let mut units = collect_py_units(py_files).expect("python stats units");
    units.extend(collect_rs_units(rs_files).expect("rust stats units"));
    units
}

fn collect_py_units(py_files: &[PathBuf]) -> Result<Vec<kiss::UnitMetrics>, kiss::RoleBuildError> {
    use kiss::{build_python_context_graph, collect_detailed_py};

    let (parsed, roles) = super::load::load_production_python(py_files)?;
    let refs: Vec<_> = parsed.iter().collect();
    let graph = build_python_context_graph(&refs, &roles).production_view();
    let mut units = collect_detailed_py(&refs, Some(&graph));
    append_cycle_units(&mut units, &graph);
    Ok(units)
}

fn collect_rs_units(rs_files: &[PathBuf]) -> Result<Vec<kiss::UnitMetrics>, kiss::RoleBuildError> {
    use kiss::{build_rust_dependency_graph_with_roles, collect_detailed_rs_with_roles};

    let (parsed, roles) = super::load::load_production_rust(rs_files)?;
    let refs: Vec<_> = parsed.iter().collect();
    let graph = build_rust_dependency_graph_with_roles(&refs, Some(&roles));
    let mut units = collect_detailed_rs_with_roles(&refs, Some(&graph), Some(&roles));
    append_cycle_units(&mut units, &graph);
    Ok(units)
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
