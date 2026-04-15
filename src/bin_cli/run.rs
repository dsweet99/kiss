use crate::bin_cli::args::{Cli, Commands};
use crate::bin_cli::config_session::{
    ensure_default_config_exists, load_configs, load_gate_config, run_init_command,
};
use crate::bin_cli::dispatch::dispatch;
use clap::Parser;

pub fn run() -> i32 {
    let cli = Cli::parse();
    if let Commands::Init { repo_path } = &cli.command {
        return run_init_command(repo_path);
    }
    ensure_default_config_exists();
    let (py_config, rs_config) = load_configs(cli.config.as_ref(), cli.defaults);
    let gate_config = load_gate_config(cli.config.as_ref(), cli.defaults);
    dispatch(cli, &py_config, &rs_config, &gate_config)
}
