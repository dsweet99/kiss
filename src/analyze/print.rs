use kiss::cli_output::{print_duplicates, print_final_status, print_violations};
use kiss::{DependencyGraph, DuplicateCluster, Violation};

pub(crate) fn print_analysis_summary(
    metrics: &kiss::GlobalMetrics,
    py_g: Option<&DependencyGraph>,
    rs_g: Option<&DependencyGraph>,
) {
    use crate::analyze::graph_api::graph_stats;
    let (nodes, edges) = graph_stats(py_g, rs_g);
    println!(
        "Analyzed: {} files, {} code_units, {} statements, {nodes} graph_nodes, {edges} graph_edges",
        metrics.files, metrics.code_units, metrics.statements,
    );
}

pub(crate) struct PrintResultsCtx {
    pub show_timing: bool,
    pub t_phase2: Option<std::time::Instant>,
    pub suppress_final_status: bool,
}

pub(crate) fn print_all_results_with_dups(
    viols: &[Violation],
    py_dups: &[DuplicateCluster],
    rs_dups: &[DuplicateCluster],
    ctx: PrintResultsCtx,
) -> bool {
    let t1 = std::time::Instant::now();
    let dup_count = py_dups.len() + rs_dups.len();

    print_violations(viols);
    print_duplicates("Python", py_dups);
    print_duplicates("Rust", rs_dups);
    if ctx.show_timing
        && let Some(t0) = ctx.t_phase2
    {
        let t2 = std::time::Instant::now();
        eprintln!(
            "[TIMING] dup_detect={:.2}s, output={:.2}s",
            t1.duration_since(t0).as_secs_f64(),
            t2.duration_since(t1).as_secs_f64()
        );
    }

    let has_violations = !viols.is_empty() || dup_count > 0;
    if !ctx.suppress_final_status {
        print_final_status(has_violations);
    }

    !has_violations
}

pub(crate) fn log_parse_timing(show: bool, timing: &str) {
    if show && !timing.is_empty() {
        eprintln!("[TIMING] {timing}");
    }
}

pub(crate) fn log_timing_phase2(show: bool, t3: std::time::Instant, t4: std::time::Instant) {
    if show {
        eprintln!(
            "[TIMING] graph_analysis={:.2}s, test_refs={:.2}s",
            t4.duration_since(t3).as_secs_f64(),
            std::time::Instant::now().duration_since(t4).as_secs_f64()
        );
    }
}

pub(crate) fn log_timing_phase1(
    t0: std::time::Instant,
    t1: std::time::Instant,
    t2: std::time::Instant,
    t3: std::time::Instant,
) {
    eprintln!(
        "[TIMING] discovery={:.2}s, parse+analyze={:.2}s, coverage=0.00s, graph={:.2}s",
        t1.duration_since(t0).as_secs_f64(),
        t2.duration_since(t1).as_secs_f64(),
        t3.duration_since(t2).as_secs_f64()
    );
}
