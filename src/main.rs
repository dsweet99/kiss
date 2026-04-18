#![allow(clippy::redundant_pub_crate)]
// CLI/analyze use owned "context" structs at API boundaries; pedantic prefers references everywhere.
#![allow(clippy::needless_pass_by_value)]

mod analyze;
mod analyze_cache;
mod analyze_parse;
mod bin_cli;
#[cfg(test)]
mod layout;
mod rules;
mod show_tests;
mod viz;
mod viz_coarsen;

use crate::bin_cli::{run, set_sigpipe_default};

fn main() {
    let t0 = std::time::Instant::now();
    set_sigpipe_default();
    let exit_code = run();
    let d = t0.elapsed();
    if d.as_secs() >= 1 {
        eprintln!("kiss: {:.2}s", d.as_secs_f64());
    } else {
        eprintln!("kiss: {}ms", d.as_millis());
    }
    std::process::exit(exit_code);
}
