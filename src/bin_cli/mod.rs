//! CLI wiring for the `kiss` binary (subcommands, config loading, dispatch).

pub mod args;
mod check_cmd;
mod config_session;
mod cov_cmd;
mod cov_sibling_gates;
mod cov_workspace_files;
mod cov_warm;
pub mod dispatch;
mod mimic;
mod run;
mod shrink;
mod shrink_analysis_types;
mod shrink_types;
pub mod stats;
mod test_cmd;
pub mod util;

pub use run::run_cli_entrypoint as run;
pub use util::set_sigpipe_default;

#[cfg(test)]
#[path = "gates_core.rs"]
mod gates_core;
#[cfg(test)]
#[path = "gates_shrink.rs"]
mod gates_shrink;
#[cfg(test)]
mod tests_touch;
