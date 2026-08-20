use kiss::{Config, GateConfig, Language};

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
    pub suppress_final_status: bool,
}

#[derive(Debug, Clone)]
pub struct AnalyzeResult {
    pub success: bool,
}

#[cfg(test)]
mod options_tests {
    use super::*;

    #[test]
    fn analyze_result_carries_success_flag() {
        let result = AnalyzeResult { success: true };
        assert!(result.success);
    }
}
