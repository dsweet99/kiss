//! Aggregate distributions for `kiss stats` and mimic-style config generation.

mod collect_py;
mod collect_rust;
mod definitions;
mod format;
mod metric_stats;
mod percentile;
mod summaries;

#[cfg(test)]
mod tests;

pub use definitions::{METRICS, MetricDef, MetricScope, get_metric_def};
pub use format::{format_stats_table, generate_config_toml};
pub use metric_stats::MetricStats;
pub use percentile::PercentileSummary;
pub use summaries::compute_summaries;
