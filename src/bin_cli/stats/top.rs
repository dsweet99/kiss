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

/// Registry metric IDs that intentionally have NO per-unit extractor.
///
/// `kiss stats --all` is a per-unit "top-N outliers" view. A few registry
/// metrics are graph-aggregates that don't map to any single unit (file,
/// function, class, or module), so they cannot appear in `--all` output.
///
/// This list is consumed only by the exhaustiveness invariants in
/// `exhaustiveness_tests` below; it carries no runtime effect, so it's marked
/// `dead_code` for production builds. Every entry MUST satisfy two invariants:
///   1. The ID exists in `kiss::METRICS` (catches stale entries on rename/remove).
///   2. `extractor_for` returns `None` for it (no double-defining).
///
/// If you add a new metric to `kiss::METRICS`, you must EITHER add an arm to
/// `extractor_for` OR add the ID here with a comment explaining why it has no
/// per-unit representation. The test will fail otherwise.
#[allow(dead_code)]
const AGGREGATE_ONLY_METRICS: &[&str] = &[
    // `cycle_size` measures the size of a strongly-connected component in the
    // module dependency graph. A cycle is not a unit (file/function/class/module),
    // so there is no UnitMetrics field to extract. Cycles surface in
    // `kiss check` violations, not in `--all` outliers.
    "cycle_size",
    // `inv_test_coverage` is computed only by the summary path
    // (`bin_cli/stats/summary.rs`), which runs the project-wide test-reference
    // analysis (`analyze_test_refs` / `analyze_rust_test_refs`) needed to derive
    // per-file coverage percentages. The `--all` path is purely a per-unit
    // outlier ranker over `UnitMetrics` and does not (currently) run that
    // analysis, so it has no coverage value to rank. Wiring it up would require
    // running the test-ref scan from `top.rs` and populating a new
    // `UnitMetrics.inv_test_coverage` field; left for a follow-up.
    "inv_test_coverage",
];

/// Map a canonical registry metric ID (`kiss::METRICS`) to the `UnitMetrics`
/// field that carries its per-unit value.
///
/// Returns `None` for registry metrics listed in `AGGREGATE_ONLY_METRICS`.
fn extractor_for(metric_id: &str) -> Option<UnitMetricExtractor> {
    match metric_id {
        // function-scope
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
        // type-scope
        "methods_per_class" => Some(|u| u.methods),
        // file-scope
        "statements_per_file" => Some(|u| u.file_statements),
        "lines_per_file" => Some(|u| u.lines),
        "functions_per_file" => Some(|u| u.file_functions),
        "interface_types_per_file" => Some(|u| u.interface_types),
        "concrete_types_per_file" => Some(|u| u.concrete_types),
        "imported_names_per_file" => Some(|u| u.imports),
        // module-scope
        "fan_in" => Some(|u| u.fan_in),
        "fan_out" => Some(|u| u.fan_out),
        "indirect_dependencies" => Some(|u| u.indirect_deps),
        "dependency_depth" => Some(|u| u.dependency_depth),
        // Anything else: not yet wired. The test below will fail until either an
        // extractor arm is added or the ID is moved to AGGREGATE_ONLY_METRICS.
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

#[cfg(test)]
mod exhaustiveness_tests {
    use super::{AGGREGATE_ONLY_METRICS, extractor_for};

    /// Invariant: every metric in `kiss::METRICS` is either emittable by
    /// `kiss stats --all` (has an `extractor_for` arm) or is documented in
    /// `AGGREGATE_ONLY_METRICS` as having no per-unit representation.
    ///
    /// When this fails after adding a new metric to `src/stats/definitions.rs`,
    /// the fix is to either:
    ///   * add an arm to `extractor_for` mapping the new ID to the appropriate
    ///     `UnitMetrics` field (widening the struct first if needed), OR
    ///   * add the new ID to `AGGREGATE_ONLY_METRICS` with a comment explaining
    ///     why it isn't a per-unit metric.
    #[test]
    fn extractor_or_allowlist_covers_every_registry_metric() {
        let unhandled: Vec<&'static str> = kiss::METRICS
            .iter()
            .map(|m| m.metric_id)
            .filter(|id| extractor_for(id).is_none() && !AGGREGATE_ONLY_METRICS.contains(id))
            .collect();
        assert!(
            unhandled.is_empty(),
            "kiss::METRICS contains IDs with neither an extractor in `extractor_for` \
             nor an entry in `AGGREGATE_ONLY_METRICS`: {unhandled:?}\n\
             Either wire the metric into UnitMetrics + extractor_for so `kiss stats --all` \
             can rank it, or add it to AGGREGATE_ONLY_METRICS with a comment explaining why."
        );
    }

    /// Mutual exclusivity: an ID in the allow-list must NOT also have an extractor.
    /// Prevents the allow-list from going stale when a previously aggregate-only
    /// metric is later given a per-unit field.
    #[test]
    fn allowlist_entries_have_no_extractor() {
        let conflicting: Vec<&'static str> = AGGREGATE_ONLY_METRICS
            .iter()
            .copied()
            .filter(|id| extractor_for(id).is_some())
            .collect();
        assert!(
            conflicting.is_empty(),
            "AGGREGATE_ONLY_METRICS lists IDs that ALSO have extractors in `extractor_for`: \
             {conflicting:?}\n\
             Remove these from AGGREGATE_ONLY_METRICS — the registry-driven loop in \
             print_all_top_metrics already emits them via extractor_for."
        );
    }

    /// Allow-list entries must reference real registry metrics. Catches typos
    /// and stale entries when a metric is removed or renamed in
    /// `src/stats/definitions.rs`.
    #[test]
    fn allowlist_entries_exist_in_registry() {
        let registry_ids: Vec<&'static str> =
            kiss::METRICS.iter().map(|m| m.metric_id).collect();
        let stale: Vec<&'static str> = AGGREGATE_ONLY_METRICS
            .iter()
            .copied()
            .filter(|id| !registry_ids.contains(id))
            .collect();
        assert!(
            stale.is_empty(),
            "AGGREGATE_ONLY_METRICS contains IDs that are not in kiss::METRICS: {stale:?}\n\
             registry: {registry_ids:?}"
        );
    }
}
