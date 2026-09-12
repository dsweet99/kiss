use std::path::PathBuf;

#[cfg(not(test))]
use std::path::Path;
#[cfg(not(test))]
use std::process::{Command, Output, Stdio};

#[cfg(not(test))]
use crate::bin_cli::check_cmd::CheckCommandArgs;
#[cfg(not(test))]
use kiss::Language;

pub const SHARD_ENV: &str = "KISS_CHECK_GATHER_ROOTS";
const MAX_SHARDS: usize = 8;

pub(crate) fn gather_roots_from_env() -> Option<Vec<PathBuf>> {
    let raw = std::env::var_os(SHARD_ENV)?;
    let text = raw.to_string_lossy();
    let roots: Vec<PathBuf> = text
        .split('\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    if roots.is_empty() {
        None
    } else {
        Some(roots)
    }
}

#[cfg(not(test))]
pub(crate) fn run_split_check_sharded(exe: &Path, args: &CheckCommandArgs<'_>) -> i32 {
    let ignore = crate::bin_cli::util::merge_check_ignore_prefixes(args.ignore);
    let universe = Path::new(&args.paths[0]);
    let (_, rs_files) = crate::analyze::gather_files(universe, Some(Language::Rust), &ignore);
    let shard_count = rust_shard_count(rs_files.len());
    if shard_count <= 1 || rs_files.len() < 64 {
        return crate::bin_cli::check_cmd::run_split_check_with_exe_legacy(exe, args);
    }
    let shards = partition_paths(rs_files, shard_count);
    let Ok(mut python) = spawn_lang(exe, args, "python", None) else {
        return crate::bin_cli::check_cmd::run_check_in_process_pub(args);
    };
    let mut rust_children = Vec::with_capacity(shards.len());
    for shard in &shards {
        match spawn_lang(exe, args, "rust", Some(shard)) {
            Ok(child) => rust_children.push(child),
            Err(_) => {
                let _ = python.kill();
                for child in &mut rust_children {
                    let _ = child.kill();
                }
                return crate::bin_cli::check_cmd::run_check_in_process_pub(args);
            }
        }
    }
    let python_out = python.wait_with_output().ok();
    let mut rust_outs = Vec::with_capacity(rust_children.len());
    for child in rust_children {
        match child.wait_with_output() {
            Ok(out) => rust_outs.push(out),
            Err(_) => {
                return crate::bin_cli::check_cmd::run_check_in_process_pub(args);
            }
        }
    }
    match python_out {
        Some(python) => publish_sharded_outputs(&python, &rust_outs),
        None => crate::bin_cli::check_cmd::run_check_in_process_pub(args),
    }
}

fn rust_shard_count(file_count: usize) -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, MAX_SHARDS);
    cpus.min(file_count.max(1)).max(1)
}

fn partition_paths(mut paths: Vec<PathBuf>, shards: usize) -> Vec<Vec<PathBuf>> {
    paths.sort();
    let mut bins: Vec<Vec<PathBuf>> = (0..shards).map(|_| Vec::new()).collect();
    for (idx, path) in paths.into_iter().enumerate() {
        bins[idx % shards].push(path);
    }
    bins.into_iter().filter(|b| !b.is_empty()).collect()
}

#[cfg(not(test))]
fn spawn_lang(
    exe: &Path,
    args: &CheckCommandArgs<'_>,
    lang: &str,
    gather_roots: Option<&[PathBuf]>,
) -> std::io::Result<std::process::Child> {
    let mut cmd = Command::new(exe);
    cmd.arg("check").arg("--lang").arg(lang);
    if args.timing {
        cmd.arg("--timing");
    }
    if let Some(path) = args.config {
        cmd.arg("--config").arg(path);
    }
    for prefix in args.ignore {
        cmd.arg("--ignore").arg(prefix);
    }
    cmd.args(args.paths)
        .env("KISS_CHECK_WORKER", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(roots) = gather_roots {
        let joined = roots
            .iter()
            .map(|p| p.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        cmd.env(SHARD_ENV, joined);
    }
    cmd.spawn()
}

#[cfg(not(test))]
fn publish_sharded_outputs(python: &Output, rust_outs: &[Output]) -> i32 {
    crate::bin_cli::check_cmd::forward_worker_stderr_pub(&python.stderr);
    for rust in rust_outs {
        crate::bin_cli::check_cmd::forward_worker_stderr_pub(&rust.stderr);
    }
    let mut totals = [0_usize; 5];
    let mut ok = python.status.success();
    for out in std::iter::once(python).chain(rust_outs.iter()) {
        ok &= out.status.success();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some(next) = crate::bin_cli::check_cmd::analyzed_add_pub(totals, line) {
                totals = next;
            } else if line != "NO VIOLATIONS" {
                println!("{line}");
            }
        }
    }
    println!(
        "Analyzed: {} files, {} code_units, {} statements, {} graph_nodes, {} graph_edges",
        totals[0], totals[1], totals[2], totals[3], totals[4]
    );
    if ok {
        println!("NO VIOLATIONS");
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_paths_spreads_evenly() {
        let paths: Vec<PathBuf> = (0..10).map(|i| PathBuf::from(format!("f{i}.rs"))).collect();
        let bins = partition_paths(paths, 4);
        assert_eq!(bins.len(), 4);
        assert!(bins.iter().all(|b| (2..=3).contains(&b.len())));
    }

    #[test]
    fn rust_shard_count_clamps() {
        assert_eq!(rust_shard_count(0), 1);
        assert!(rust_shard_count(10_000) <= MAX_SHARDS);
    }
}
