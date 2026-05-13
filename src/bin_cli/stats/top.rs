use crate::bin_cli::config_session::config_provenance;
use kiss::check_universe_cache::CachedCoverageItem;
use kiss::{Config, GateConfig, Language};
use std::collections::HashMap;
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

type FreshCoverageItems = (Vec<CachedCoverageItem>, Vec<CachedCoverageItem>);

pub fn run_stats_top(args: StatsTopArgs<'_>) {
    let (py_files, rs_files) =
        kiss::discovery::gather_files_by_lang(args.paths, args.lang_filter, args.ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        eprintln!("No source files found.");
        std::process::exit(1);
    }
    println!(
        "kiss stats --all {n} - Top Outliers\nAnalyzed from: {paths}\n{prov}\n",
        n = args.n,
        paths = args.paths.join(", "),
        prov = config_provenance()
    );
    let cached_coverage = crate::analyze_cache::try_run_cached_stats_top(
        &py_files,
        &rs_files,
        args.py_config,
        args.rs_config,
        args.gate_config,
    )
    .map(coverage_map_to_string_keys);
    let (py_units, py_fresh) = collect_py_units(&py_files, cached_coverage.as_ref());
    let (rs_units, rs_fresh) = collect_rs_units(&rs_files, cached_coverage.as_ref());
    if cached_coverage.is_none()
        && let Some((definitions, unreferenced)) = merge_fresh_items(py_fresh, rs_fresh)
    {
        crate::analyze_cache::maybe_store_stats_top_cache(
            &py_files,
            &rs_files,
            args.py_config,
            args.rs_config,
            args.gate_config,
            definitions,
            unreferenced,
        );
    }
    let mut all_units = py_units;
    all_units.extend(rs_units);
    print_all_top_metrics(&all_units, args.n);
}

fn coverage_map_to_string_keys(map: HashMap<PathBuf, usize>) -> HashMap<String, usize> {
    map.into_iter()
        .map(|(p, v)| (p.display().to_string(), v))
        .collect()
}

fn merge_fresh_items(
    py: Option<FreshCoverageItems>,
    rs: Option<FreshCoverageItems>,
) -> Option<FreshCoverageItems> {
    if py.is_none() && rs.is_none() {
        return None;
    }
    let mut defs = Vec::new();
    let mut unrefs = Vec::new();
    for (d, u) in py.into_iter().chain(rs) {
        defs.extend(d);
        unrefs.extend(u);
    }
    Some((defs, unrefs))
}

#[cfg(test)]
pub fn collect_all_units(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    cached_coverage: Option<&HashMap<String, usize>>,
) -> Vec<kiss::UnitMetrics> {
    let (py_units, _) = collect_py_units(py_files, cached_coverage);
    let (rs_units, _) = collect_rs_units(rs_files, cached_coverage);
    let mut units = py_units;
    units.extend(rs_units);
    units
}

fn collect_py_units(
    py_files: &[PathBuf],
    cached_coverage: Option<&HashMap<String, usize>>,
) -> (Vec<kiss::UnitMetrics>, Option<FreshCoverageItems>) {
    use kiss::parsing::parse_files;
    use kiss::{analyze_test_refs, build_dependency_graph, collect_detailed_py};

    collect_lang_units(LangCollect {
        files: py_files,
        cached_coverage,
        parse: |files| match parse_files(files) {
            Ok(r) => r.into_iter().filter_map(Result::ok).collect(),
            Err(e) => {
                eprintln!("error: failed to parse Python files: {e}");
                Vec::new()
            }
        },
        build_graph: build_dependency_graph,
        analyze: |refs, graph| {
            let cov = analyze_test_refs(refs, Some(graph));
            (cov.definitions, cov.unreferenced)
        },
        collect_detailed: collect_detailed_py,
        file_of: |d: &kiss::CodeDefinition| &d.file,
        item_of: |d: &kiss::CodeDefinition| CachedCoverageItem {
            file: d.file.to_string_lossy().to_string(),
            name: d.name.clone(),
            line: d.line,
        },
    })
}

fn collect_rs_units(
    rs_files: &[PathBuf],
    cached_coverage: Option<&HashMap<String, usize>>,
) -> (Vec<kiss::UnitMetrics>, Option<FreshCoverageItems>) {
    use kiss::rust_graph::build_rust_dependency_graph;
    use kiss::rust_parsing::parse_rust_files;
    use kiss::{analyze_rust_test_refs, collect_detailed_rs};

    collect_lang_units(LangCollect {
        files: rs_files,
        cached_coverage,
        parse: |files| {
            parse_rust_files(files)
                .into_iter()
                .filter_map(Result::ok)
                .collect()
        },
        build_graph: build_rust_dependency_graph,
        analyze: |refs, graph| {
            let cov = analyze_rust_test_refs(refs, Some(graph));
            (cov.definitions, cov.unreferenced)
        },
        collect_detailed: collect_detailed_rs,
        file_of: |d: &kiss::RustCodeDefinition| &d.file,
        item_of: |d: &kiss::RustCodeDefinition| CachedCoverageItem {
            file: d.file.to_string_lossy().to_string(),
            name: d.name.clone(),
            line: d.line,
        },
    })
}

struct LangCollect<'a, P, D, FParse, FBuild, FAnalyze, FCollect, FFile, FItem>
where
    FParse: FnOnce(&[PathBuf]) -> Vec<P>,
    FBuild: FnOnce(&[&P]) -> kiss::DependencyGraph,
    FAnalyze: FnOnce(&[&P], &kiss::DependencyGraph) -> (Vec<D>, Vec<D>),
    FCollect: FnOnce(&[&P], Option<&kiss::DependencyGraph>) -> Vec<kiss::UnitMetrics>,
    FFile: Fn(&D) -> &PathBuf,
    FItem: Fn(&D) -> CachedCoverageItem,
{
    files: &'a [PathBuf],
    cached_coverage: Option<&'a HashMap<String, usize>>,
    parse: FParse,
    build_graph: FBuild,
    analyze: FAnalyze,
    collect_detailed: FCollect,
    file_of: FFile,
    item_of: FItem,
}

fn collect_lang_units<P, D, FParse, FBuild, FAnalyze, FCollect, FFile, FItem>(
    args: LangCollect<'_, P, D, FParse, FBuild, FAnalyze, FCollect, FFile, FItem>,
) -> (Vec<kiss::UnitMetrics>, Option<FreshCoverageItems>)
where
    FParse: FnOnce(&[PathBuf]) -> Vec<P>,
    FBuild: FnOnce(&[&P]) -> kiss::DependencyGraph,
    FAnalyze: FnOnce(&[&P], &kiss::DependencyGraph) -> (Vec<D>, Vec<D>),
    FCollect: FnOnce(&[&P], Option<&kiss::DependencyGraph>) -> Vec<kiss::UnitMetrics>,
    FFile: Fn(&D) -> &PathBuf,
    FItem: Fn(&D) -> CachedCoverageItem,
{
    if args.files.is_empty() {
        return (Vec::new(), None);
    }
    let parsed = (args.parse)(args.files);
    let parsed_refs: Vec<&P> = parsed.iter().collect();
    let graph = (args.build_graph)(&parsed_refs);
    let (coverage_map, fresh) = if let Some(m) = args.cached_coverage {
        (m.clone(), None)
    } else {
        let (defs, unrefs) = (args.analyze)(&parsed_refs, &graph);
        let map = coverage_pct_map(&defs, &unrefs, &args.file_of);
        let cached_defs: Vec<CachedCoverageItem> = defs.iter().map(&args.item_of).collect();
        let cached_unrefs: Vec<CachedCoverageItem> = unrefs.iter().map(&args.item_of).collect();
        (map, Some((cached_defs, cached_unrefs)))
    };
    let mut units = (args.collect_detailed)(&parsed_refs, Some(&graph));
    decorate_file_units_with_coverage(&mut units, &coverage_map);
    append_cycle_units(&mut units, &graph);
    (units, fresh)
}

pub(super) fn coverage_pct_map<D, F>(defs: &[D], unrefs: &[D], file_of: F) -> HashMap<String, usize>
where
    F: Fn(&D) -> &PathBuf,
{
    coverage_map_to_string_keys(kiss::cli_output::file_coverage_map_from_paths(
        defs.iter().map(&file_of),
        unrefs.iter().map(&file_of),
    ))
}

pub(super) fn decorate_file_units_with_coverage(
    units: &mut [kiss::UnitMetrics],
    coverage_map: &HashMap<String, usize>,
) {
    for u in units.iter_mut().filter(|u| u.kind == "file") {
        let coverage_pct = coverage_map.get(&u.file).copied().unwrap_or(100);
        u.inv_test_coverage = Some(100usize.saturating_sub(coverage_pct));
    }
}

pub(super) fn append_cycle_units(units: &mut Vec<kiss::UnitMetrics>, graph: &kiss::DependencyGraph) {
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

type UnitMetricExtractor = fn(&kiss::UnitMetrics) -> Option<usize>;

#[cfg(test)]
pub(super) const AGGREGATE_ONLY_METRICS: &[&str] = &[];

pub(super) fn extractor_for(metric_id: &str) -> Option<UnitMetricExtractor> {
    match metric_id {
        "statements_per_function" => Some(|u| u.statements),
        "arguments_per_function" => Some(|u| u.arguments),
        "positional_args" => Some(|u| u.args_positional),
        "keyword_only_args" => Some(|u| u.args_keyword_only),
        "max_indentation_depth" => Some(|u| u.indentation),
        "nested_function_depth" => Some(|u| u.nested_depth),
        "returns_per_function" => Some(|u| u.returns),
        "return_values_per_function" => Some(|u| u.return_values),
        "branches_per_function" => Some(|u| u.branches),
        "local_variables_per_function" => Some(|u| u.locals),
        "statements_per_try_block" => Some(|u| u.try_block_statements),
        "boolean_parameters" => Some(|u| u.boolean_parameters),
        "annotations_per_function" => Some(|u| u.annotations),
        "calls_per_function" => Some(|u| u.calls),
        "methods_per_class" => Some(|u| u.methods),
        "statements_per_file" => Some(|u| u.file_statements),
        "lines_per_file" => Some(|u| u.lines),
        "functions_per_file" => Some(|u| u.file_functions),
        "interface_types_per_file" => Some(|u| u.interface_types),
        "concrete_types_per_file" => Some(|u| u.concrete_types),
        "imported_names_per_file" => Some(|u| u.imports),
        "inv_test_coverage" => Some(|u| u.inv_test_coverage),
        "fan_in" => Some(|u| u.fan_in),
        "fan_out" => Some(|u| u.fan_out),
        "indirect_dependencies" => Some(|u| u.indirect_deps),
        "dependency_depth" => Some(|u| u.dependency_depth),
        "cycle_size" => Some(|u| u.cycle_size),
        _ => None,
    }
}

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

