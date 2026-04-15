use crate::analyze;

use super::shrink_types::ShrinkFullContext;

pub struct ShrinkAnalyzeArgs<'a, 'c> {
    pub paths: &'a [String],
    pub ignore: &'a [String],
    pub ctx: &'c ShrinkFullContext<'c>,
}

pub struct ShrinkMetricsArgs<'a, 'c> {
    pub result: &'a analyze::AnalyzeResult,
    pub paths: &'a [String],
    pub ignore: &'a [String],
    pub ctx: &'c ShrinkFullContext<'c>,
}

#[cfg(test)]
mod shrink_analysis_types_coverage {
    use super::*;

    #[test]
    fn touch_analysis_arg_types() {
        let _ = std::mem::size_of::<ShrinkAnalyzeArgs>();
        let _ = std::mem::size_of::<ShrinkMetricsArgs>();
    }
}
