use kiss::check_universe_cache::CachedCoverageItem;
use kiss::{DuplicateCluster, Violation};

use crate::analyze::options::AnalyzeOptions;
use crate::analyze_parse::ParseResult;

pub(crate) struct FullCacheStoreInput<'a> {
    pub opts: &'a AnalyzeOptions<'a>,
    pub py_files: &'a [std::path::PathBuf],
    pub rs_files: &'a [std::path::PathBuf],
    pub result: &'a ParseResult,
    pub graph_viols_all: &'a [Violation],
    pub coverage_violations: &'a [Violation],
    pub py_graph: Option<&'a kiss::DependencyGraph>,
    pub rs_graph: Option<&'a kiss::DependencyGraph>,
    pub py_dups_all: &'a [DuplicateCluster],
    pub rs_dups_all: &'a [DuplicateCluster],
    pub coverage_cache_lists: Option<(Vec<CachedCoverageItem>, Vec<CachedCoverageItem>)>,
}

pub(crate) fn maybe_store_full_cache(inp: FullCacheStoreInput<'_>) {
    // Cache writes are independent of `--all`: every successful `kiss check`
    // run primes the cache so subsequent invocations (with or without
    // `--all`) can hit it. We still skip writes when the user asked for
    // timing breakdowns, so the timed run isn't influenced by I/O it would
    // not normally do.
    if inp.opts.show_timing {
        return;
    }
    let Some((definitions, unreferenced)) = inp.coverage_cache_lists else {
        return;
    };
    let fp = crate::analyze_cache::fingerprint_for_check(
        inp.py_files,
        inp.rs_files,
        inp.opts.py_config,
        inp.opts.rs_config,
        inp.opts.gate_config,
    );
    crate::analyze_cache::store_full_cache_from_run(crate::analyze_cache::FullCacheInputs {
        fingerprint: fp,
        py_file_count: inp.result.py_parsed.len(),
        rs_file_count: inp.result.rs_parsed.len(),
        code_unit_count: inp.result.code_unit_count,
        statement_count: inp.result.statement_count,
        violations: &inp.result.violations,
        graph_viols_all: inp.graph_viols_all,
        coverage_violations: inp.coverage_violations,
        py_graph: inp.py_graph,
        rs_graph: inp.rs_graph,
        py_dups_all: inp.py_dups_all,
        rs_dups_all: inp.rs_dups_all,
        definitions,
        unreferenced,
    });
}
