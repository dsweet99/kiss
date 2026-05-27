use kiss::check_universe_cache::CachedCoverageItem;
use std::collections::HashMap;
use std::path::PathBuf;

use super::top::{
    append_cycle_units, coverage_pct_map, decorate_file_units_with_coverage, FreshCoverageItems,
};

pub(super) fn collect_py_units(
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

pub(super) fn collect_rs_units(
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

pub(crate) struct LangCollect<'a, P, D, FParse, FBuild, FAnalyze, FCollect, FFile, FItem>
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

pub(crate) fn collect_lang_units<P, D, FParse, FBuild, FAnalyze, FCollect, FFile, FItem>(
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

#[cfg(test)]
mod collect_tests {
    use super::*;
    use kiss::check_universe_cache::CachedCoverageItem;

    #[test]
    fn collect_lang_units_direct_empty() {
        let files: Vec<PathBuf> = Vec::new();
        let args = LangCollect {
            files: &files,
            cached_coverage: None,
            parse: |_files| Vec::<kiss::parsing::ParsedFile>::new(),
            build_graph: |_refs| kiss::DependencyGraph::new(),
            analyze: |_refs, _graph| {
                (
                    Vec::<kiss::CodeDefinition>::new(),
                    Vec::<kiss::CodeDefinition>::new(),
                )
            },
            collect_detailed: |_refs, _graph| Vec::new(),
            file_of: |d: &kiss::CodeDefinition| &d.file,
            item_of: |d: &kiss::CodeDefinition| CachedCoverageItem {
                file: d.file.to_string_lossy().to_string(),
                name: d.name.clone(),
                line: d.line,
            },
        };
        let (units, fresh) = collect_lang_units(args);
        assert!(units.is_empty());
        assert!(fresh.is_none());
    }
}
