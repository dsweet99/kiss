//! Output emission for cache-hit paths. Split out of `mod.rs` to keep the
//! `analyze_cache` module under the per-file size threshold.
use crate::analyze::FocusFilter;
use kiss::check_cache::CachedViolation;
use kiss::check_universe_cache::FullCheckCache;
use kiss::cli_output::{print_duplicates, print_final_status, print_violations};
use std::io::Write;
use std::path::Path;

use super::{cached_coverage_viols, cached_duplicates};
use crate::analyze::evaluate_cached_gate;

pub(super) fn emit_cached_bypass(
    cache: FullCheckCache,
    opts: &crate::analyze::AnalyzeOptions<'_>,
    focus: &FocusFilter,
) -> bool {
    let (_viols, py_dups, rs_dups, cache) = cached_duplicates(cache, opts.gate_config, focus);
    print_cached_header(&cache);
    print_cached_bypass_violations(&cache, focus);
    print_duplicates("Python", &py_dups);
    print_duplicates("Rust", &rs_dups);
    let has_violations =
        cached_bypass_has_violations(&cache, focus) || !py_dups.is_empty() || !rs_dups.is_empty();
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
    focus: &FocusFilter,
) -> bool {
    if evaluate_cached_gate(
        &cache.definitions,
        &cache.unreferenced,
        focus,
        opts.gate_config.test_coverage_threshold,
        None,
    )
    .is_some()
    {
        return false;
    }

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

fn cached_violation_in_focus(v: &CachedViolation, focus: &FocusFilter) -> bool {
    !focus.is_active() || focus.paths().contains(Path::new(&v.file))
}

fn cached_bypass_has_violations(cache: &FullCheckCache, focus: &FocusFilter) -> bool {
    cache
        .base_violations
        .iter()
        .chain(cache.graph_violations.iter())
        .any(|v| cached_violation_in_focus(v, focus))
        || !cached_coverage_viols(cache, focus).is_empty()
}

fn write_cached_violation(w: &mut impl Write, v: &CachedViolation) -> std::io::Result<()> {
    writeln!(
        w,
        "VIOLATION:{}:{}:{}:{}: {} {}",
        v.metric, v.file, v.line, v.unit_name, v.message, v.suggestion
    )
}

fn write_cached_violation_slice(
    w: &mut impl Write,
    viols: &[CachedViolation],
    focus: &FocusFilter,
) -> std::io::Result<()> {
    for v in viols {
        if cached_violation_in_focus(v, focus) {
            write_cached_violation(w, v)?;
        }
    }
    Ok(())
}

fn print_cached_bypass_violations(cache: &FullCheckCache, focus: &FocusFilter) {
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::new(stdout.lock());
    let _ = write_cached_bypass_violations(&mut w, cache, focus);
}

fn write_cached_bypass_violations(
    w: &mut impl Write,
    cache: &FullCheckCache,
    focus: &FocusFilter,
) -> std::io::Result<()> {
    let _ = write_cached_violation_slice(w, &cache.base_violations, focus);
    let _ = write_cached_violation_slice(w, &cache.graph_violations, focus);
    for v in cached_coverage_viols(cache, focus) {
        writeln!(
            w,
            "VIOLATION:{}:{}:{}:{}: {} {}",
            v.metric,
            v.file.display(),
            v.line,
            v.unit_name,
            v.message,
            v.suggestion
        )?;
    }
    Ok(())
}
