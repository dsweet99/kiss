mod summary;
mod table;
mod top;

#[cfg(test)]
pub use summary::run_stats_summary;
#[cfg(test)]
pub use table::run_stats_table;
#[cfg(test)]
pub use top::{collect_all_units, print_all_top_metrics, print_top_for_metric, run_stats_top};

use kiss::Language;

pub struct RunStatsArgs<'a> {
    pub paths: &'a [String],
    pub lang_filter: Option<Language>,
    pub ignore: &'a [String],
    pub all: Option<usize>,
    pub table: bool,
}

pub fn run_stats(args: RunStatsArgs<'_>) {
    if args.table {
        table::run_stats_table(args.paths, args.lang_filter, args.ignore);
    } else if let Some(n) = args.all {
        top::run_stats_top(args.paths, args.lang_filter, args.ignore, n);
    } else {
        summary::run_stats_summary(args.paths, args.lang_filter, args.ignore);
    }
}
