//! Output emission for cache-hit paths. Split out of `mod.rs` to keep the
//! `analyze_cache` module under the per-file size threshold.
use std::collections::HashSet;
use std::path::PathBuf;

use kiss::check_universe_cache::FullCheckCache;
use kiss::cli_output::{print_duplicates, print_final_status, print_violations};

use super::{cached_coverage_viols, cached_duplicates};
use crate::analyze::evaluate_cached_gate;

pub(super) fn emit_cached_bypass(
    cache: FullCheckCache,
    opts: &crate::analyze::AnalyzeOptions<'_>,
    focus_set: &HashSet<PathBuf>,
) -> bool {
    let (mut viols, py_dups, rs_dups, cache) =
        cached_duplicates(cache, opts.gate_config, focus_set);
    viols.extend(cached_coverage_viols(&cache, focus_set));
    print_cached_header(&cache);
    print_violations(&viols);
    print_duplicates("Python", &py_dups);
    print_duplicates("Rust", &rs_dups);
    let has_violations = !(viols.is_empty() && py_dups.is_empty() && rs_dups.is_empty());
    print_final_status(has_violations);
    !has_violations
}

/// Cached counterpart to the gated default flow: if the cached coverage data
/// would trip the `test_coverage` gate, emit `GATE_FAILED` and per-definition
/// coverage violations exactly like `evaluate_gate` does in the live path; on
/// success, emit base + graph violations + duplicates.
pub(super) fn emit_cached_gated(
    cache: FullCheckCache,
    opts: &crate::analyze::AnalyzeOptions<'_>,
    focus_set: &HashSet<PathBuf>,
) -> bool {
    if evaluate_cached_gate(
        &cache.definitions,
        &cache.unreferenced,
        focus_set,
        opts.gate_config.test_coverage_threshold,
    )
    .is_some()
    {
        return false;
    }

    let (viols, py_dups, rs_dups, cache) = cached_duplicates(cache, opts.gate_config, focus_set);
    print_cached_header(&cache);
    print_violations(&viols);
    print_duplicates("Python", &py_dups);
    print_duplicates("Rust", &rs_dups);
    let has_violations = !(viols.is_empty() && py_dups.is_empty() && rs_dups.is_empty());
    print_final_status(has_violations);
    !has_violations
}

fn print_cached_header(cache: &FullCheckCache) {
    println!(
        "Analyzed: {} files, {} code_units, {} statements, {} graph_nodes, {} graph_edges",
        cache.py_file_count + cache.rs_file_count,
        cache.code_unit_count,
        cache.statement_count,
        cache.graph_nodes,
        cache.graph_edges
    );
}
