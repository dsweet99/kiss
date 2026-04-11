use kiss::{Config, GateConfig, Language};

/// Options for a full universe analysis run.
pub struct AnalyzeOptions<'a> {
    pub universe: &'a str,
    pub focus_paths: &'a [String],
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
    pub lang_filter: Option<Language>,
    pub bypass_gate: bool,
    pub gate_config: &'a GateConfig,
    pub ignore_prefixes: &'a [String],
    pub show_timing: bool,
    /// If true, suppress "NO VIOLATIONS" sentinel (used by shrink mode after constraint check)
    pub suppress_final_status: bool,
}

/// Result of running analysis, including computed global metrics.
#[derive(Debug, Clone)]
pub struct AnalyzeResult {
    /// Whether the analysis passed (no violations).
    pub success: bool,
    /// Global metrics computed during analysis.
    /// `None` on cache hit (metrics not recomputed); `Some` on full analysis.
    pub metrics: Option<kiss::GlobalMetrics>,
}
