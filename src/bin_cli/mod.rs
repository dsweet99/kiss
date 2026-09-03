pub mod args;
mod check_cmd;
mod config_session;
mod cov_cmd;
mod cov_cmd_cache;
mod cov_sibling_gates;
mod cov_warm;
mod cov_workspace_files;
pub mod dispatch;
mod mimic;
mod run;
pub mod stats;
mod test_cmd;
#[cfg(test)]
pub(crate) use test_cmd::{TestCommandArgs, finish_with_coverage};
pub mod util;

pub use run::run_cli_entrypoint as run;
pub use util::set_sigpipe_default;

#[cfg(test)]
#[path = "gates_core.rs"]
mod gates_core;
#[cfg(test)]
mod tests_touch;
