use crate::analyze::FocusFilter;
use kiss::check_universe_cache::FullCheckCache;
use kiss::cli_output::{print_duplicates, print_final_status, print_violations};

use super::cached_duplicates;

pub(super) fn emit_cached_bypass(
    cache: FullCheckCache,
    opts: &crate::analyze::AnalyzeOptions<'_>,
    focus: &FocusFilter,
) -> bool {
    let (viols, py_dups, rs_dups, cache) = cached_duplicates(cache, opts.gate_config, focus);
    print_cached_header(&cache);
    print_violations(&viols);
    print_duplicates("Python", &py_dups);
    print_duplicates("Rust", &rs_dups);
    let has_violations = !(viols.is_empty() && py_dups.is_empty() && rs_dups.is_empty());
    print_final_status(has_violations);
    !has_violations
}

pub(super) fn emit_cached_gated(
    cache: FullCheckCache,
    opts: &crate::analyze::AnalyzeOptions<'_>,
    focus: &FocusFilter,
) -> bool {
    let (viols, py_dups, rs_dups, cache) = cached_duplicates(cache, opts.gate_config, focus);
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
