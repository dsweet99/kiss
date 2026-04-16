//! CLI wiring for the `kiss` binary (subcommands, config loading, dispatch).

mod args;
mod check_cmd;
mod config_session;
pub mod dispatch;
mod mimic;
mod run;
mod show_tests_cmd;
mod shrink;
mod shrink_analysis_types;
mod shrink_types;
pub mod stats;
mod util;

pub use run::run;
pub use util::set_sigpipe_default;

#[cfg(test)]
#[path = "gates_core.rs"]
mod gates_core;
#[cfg(test)]
#[path = "gates_shrink.rs"]
mod gates_shrink;
