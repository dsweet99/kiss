//! Config generation and mimic merge helpers.

mod collect;
mod config_keys;
mod defaults_append;
mod generate;
mod infer_gate;
mod merge;

pub use collect::{
    collect_all_stats, collect_all_stats_with_ignore, collect_py_stats,
    collect_py_stats_with_ignore, collect_rs_stats, collect_rs_stats_with_ignore,
};
pub use config_keys::{python_config_key, rust_config_key};
pub use generate::{GenerateConfigParams, generate_config_toml_by_language};
pub use infer_gate::infer_gate_config_for_paths;
pub use merge::{MergeLanguageUpdate, merge_config_toml};

use std::path::Path;

pub fn write_mimic_config(
    out: &Path,
    toml: &str,
    py_cnt: usize,
    rs_cnt: usize,
) -> Result<(), std::io::Error> {
    let content = if out.exists() {
        merge::merge_config_toml(
            out,
            toml,
            merge::MergeLanguageUpdate::from_analyzed_counts(py_cnt, rs_cnt),
        )
    } else {
        toml.to_string()
    };
    std::fs::write(out, &content)?;
    eprintln!(
        "Generated config from {} files → {}",
        py_cnt + rs_cnt,
        out.display()
    );
    Ok(())
}

#[cfg(test)]
#[path = "config_gates.rs"]
mod config_gates;
