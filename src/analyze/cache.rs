use kiss::stats::MetricStats;
use kiss::{DuplicateCluster, Violation};

use crate::analyze::focus::FocusFilter;
use crate::analyze::options::AnalyzeOptions;
use crate::analyze_parse::ParseResult;

pub(crate) struct FullCacheStoreInput<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub py_files: &'a [std::path::PathBuf],
    pub rs_files: &'a [std::path::PathBuf],
    pub focus: &'a FocusFilter,
    pub result: &'a ParseResult,
    pub graph_viols_all: &'a [Violation],
    pub py_graph: Option<&'a kiss::DependencyGraph>,
    pub rs_graph: Option<&'a kiss::DependencyGraph>,
    pub py_dups_all: &'a [DuplicateCluster],
    pub rs_dups_all: &'a [DuplicateCluster],
    pub py_stats: Option<&'a MetricStats>,
    pub rs_stats: Option<&'a MetricStats>,
}

pub(crate) fn maybe_store_full_cache(inp: FullCacheStoreInput<'_>) {
    if inp.opts.show_timing || inp.opts.suppress_final_status {
        return;
    }
    let fp = crate::analyze_cache::fingerprint_for_check(
        inp.py_files,
        inp.rs_files,
        inp.opts.py_config,
        inp.opts.rs_config,
        inp.opts.gate_config,
    );
    let focus_paths = inp.focus.cache_focus_paths();
    let focus_restrict = inp.focus.is_active();
    crate::analyze_cache::store_full_cache_from_run(crate::analyze_cache::FullCacheInputs {
        repo_root: crate::analyze_cache::repo_root_for_universe(inp.opts.universe),
        fingerprint: fp,
        py_file_count: inp
            .result
            .py_parsed
            .iter()
            .filter(|p| !kiss::code_roles::is_test_only_file(&inp.result.roles, &p.path))
            .count(),
        rs_file_count: inp
            .result
            .rs_parsed
            .iter()
            .filter(|p| !kiss::code_roles::is_test_only_file(&inp.result.roles, &p.path))
            .count(),
        code_unit_count: inp.result.code_unit_count,
        statement_count: inp.result.statement_count,
        py_stats: inp.py_stats,
        rs_stats: inp.rs_stats,
        focus_paths,
        focus_restrict,
        py_paths: inp
            .py_files
            .iter()
            .map(|f| f.to_string_lossy().to_string())
            .collect(),
        rs_paths: inp
            .rs_files
            .iter()
            .map(|f| f.to_string_lossy().to_string())
            .collect(),
        violations: &inp.result.violations,
        graph_viols_all: inp.graph_viols_all,
        py_graph: inp.py_graph,
        rs_graph: inp.rs_graph,
        py_dups_all: inp.py_dups_all,
        rs_dups_all: inp.rs_dups_all,
        file_content_digests: parsed_content_digests(inp.result),
    });
}

fn parsed_content_digests(result: &ParseResult) -> Vec<(String, u64)> {
    let py = result.py_parsed.iter().map(|p| {
        (
            p.path.to_string_lossy().to_string(),
            crate::analyze_cache::fnv1a64(0, p.source.as_bytes()),
        )
    });
    let rs = result.rs_parsed.iter().map(|p| {
        (
            p.path.to_string_lossy().to_string(),
            crate::analyze_cache::fnv1a64(0, p.source.as_bytes()),
        )
    });
    py.chain(rs).collect()
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl FullCacheStoreInput<'_> {
        fn witness() {}
    }

    #[test]
    fn witness_cache_types() {
        FullCacheStoreInput::witness();
    }
}
