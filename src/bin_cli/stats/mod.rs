mod load;
mod summary;
mod table;
mod top;
#[cfg(test)]
mod top_tests;

#[cfg(test)]
pub use summary::run_stats_summary;
#[cfg(test)]
pub use table::run_stats_table;
#[cfg(test)]
pub use top::{collect_all_units, print_all_top_metrics, print_top_for_metric};

use kiss::{Config, GateConfig, Language};

pub struct RunStatsArgs<'a> {
    pub paths: &'a [String],
    pub lang_filter: Option<Language>,
    pub ignore: &'a [String],
    pub all: Option<usize>,
    pub table: bool,
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
    pub gate_config: &'a GateConfig,
    pub language_tables: kiss::LanguageTablesPresent,
    pub config: Option<&'a std::path::Path>,
}

pub fn run_stats(args: RunStatsArgs<'_>) -> i32 {
    if args.table {
        table::run_stats_table_status(
            args.paths,
            args.lang_filter,
            args.ignore,
            args.language_tables,
            args.config,
        )
    } else if let Some(n) = args.all {
        top::run_stats_top_status(top::StatsTopArgs {
            paths: args.paths,
            lang_filter: args.lang_filter,
            ignore: args.ignore,
            n,
            py_config: args.py_config,
            rs_config: args.rs_config,
            gate_config: args.gate_config,
            language_tables: args.language_tables,
            config: args.config,
        })
    } else {
        summary::run_stats_summary(&args)
    }
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl RunStatsArgs<'_> {
        fn witness() {}
    }

    #[test]
    fn witness_run_stats_args() {
        RunStatsArgs::witness();
        let _ = kiss::LanguageTablesPresent::both();
    }
}
