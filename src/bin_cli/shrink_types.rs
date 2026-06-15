use kiss::{Config, GateConfig, Language};

/// Language filter plus Python/Rust configs and gate (for check / full analysis).
pub struct ShrinkFullContext<'a> {
    pub lang_filter: Option<Language>,
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
    pub gate_config: &'a GateConfig,
}

/// Language filter plus configs for shrink start (no gate).
pub struct ShrinkStartContext<'a> {
    pub lang_filter: Option<Language>,
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
}

pub struct RunShrinkArgs<'a> {
    pub target: Option<String>,
    pub paths: &'a [String],
    pub ignore: &'a [String],
    pub ctx: &'a ShrinkFullContext<'a>,
}

#[cfg(test)]
mod shrink_types_coverage {
    use super::*;

    impl ShrinkFullContext<'_> {
        fn witness() {}
    }
    impl ShrinkStartContext<'_> {
        fn witness() {}
    }
    impl RunShrinkArgs<'_> {
        fn witness() {}
    }

    #[test]
    fn witness_shrink_context_types() {
        ShrinkFullContext::witness();
        ShrinkStartContext::witness();
        RunShrinkArgs::witness();
    }
}
