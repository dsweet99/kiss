use crate::bin_cli::args::{Cli, Commands};
use crate::bin_cli::config_session::{ensure_default_config_exists, load_configs, load_gate_config, load_test_section_config, run_init_command};
use crate::bin_cli::dispatch::dispatch;
use crate::bin_cli::util::set_sigpipe_default;
use clap::Parser;

pub fn run() -> i32 {
    let cli = Cli::parse();
    if let Commands::Init { repo_path } = &cli.command {
        return run_init_command(repo_path);
    }
    ensure_default_config_exists();
    let (py_config, rs_config) = load_configs(cli.config.as_ref(), cli.defaults);
    let gate_config = load_gate_config(cli.config.as_ref(), cli.defaults);
    let test_section = load_test_section_config(cli.config.as_ref(), cli.defaults);
    dispatch(cli, &py_config, &rs_config, &gate_config, &test_section)
}

pub fn kiss_main_with_timing() -> i32 {
    let t0 = std::time::Instant::now();
    set_sigpipe_default();
    let exit_code = run();
    let d = t0.elapsed();
    if d.as_secs() >= 1 {
        eprintln!("kiss: {:.2}s", d.as_secs_f64());
    } else {
        eprintln!("kiss: {}ms", d.as_millis());
    }
    exit_code
}
