use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::bin_cli::check_cmd::CheckCommandArgs;
use kiss::Language;

pub const SHARD_ENV: &str = "KISS_CHECK_GATHER_ROOTS";

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
        .max(1);
    cpus.min(file_count.max(1)).max(1)
}

fn partition_paths(mut paths: Vec<PathBuf>, shards: usize) -> Vec<Vec<PathBuf>> {
    paths.sort();
    let mut weighted: Vec<(u64, PathBuf)> = paths
        .into_iter()
        .map(|path| {
            let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(1).max(1);
            (bytes, path)
        })
        .collect();
    weighted.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut bins: Vec<Vec<PathBuf>> = (0..shards).map(|_| Vec::new()).collect();
    let mut loads = vec![0_u64; shards];
    for (bytes, path) in weighted {
        let idx = loads
            .iter()
            .enumerate()
            .min_by_key(|(i, load)| (*load, *i))
            .map(|(i, _)| i)
            .unwrap_or(0);
        loads[idx] = loads[idx].saturating_add(bytes);
        bins[idx].push(path);
    }
    bins.into_iter().filter(|b| !b.is_empty()).collect()
}

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
        let tmp = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..10)
            .map(|i| {
                let path = tmp.path().join(format!("f{i}.rs"));
                std::fs::write(&path, vec![b'x'; i + 1]).unwrap();
                path
            })
            .collect();
        let bins = partition_paths(paths, 4);
        assert_eq!(bins.len(), 4);
        assert!(bins.iter().all(|b| !b.is_empty()));
    }

    #[test]
    fn partition_paths_balances_by_size() {
        let tmp = tempfile::tempdir().unwrap();
        let big = tmp.path().join("big.rs");
        let small_a = tmp.path().join("a.rs");
        let small_b = tmp.path().join("b.rs");
        std::fs::write(&big, vec![b'x'; 100]).unwrap();
        std::fs::write(&small_a, vec![b'x'; 10]).unwrap();
        std::fs::write(&small_b, vec![b'x'; 10]).unwrap();
        let bins = partition_paths(vec![big.clone(), small_a.clone(), small_b.clone()], 2);
        assert_eq!(bins.len(), 2);
        let big_shard = bins.iter().find(|b| b.contains(&big)).unwrap();
        assert_eq!(big_shard.len(), 1, "largest file should sit alone when peers are tiny");
    }

    #[test]
    fn rust_shard_count_follows_host_parallelism() {
        assert_eq!(rust_shard_count(0), 1);
        let host = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .max(1);
        assert_eq!(rust_shard_count(10_000), host);
    }

    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn sample_args(path: &String) -> CheckCommandArgs<'_> {
        CheckCommandArgs {
            paths: std::slice::from_ref(path),
            lang_filter: None,
            py_config: Box::leak(Box::new(kiss::Config::python_defaults())),
            rs_config: Box::leak(Box::new(kiss::Config::rust_defaults())),
            gate_config: Box::leak(Box::new(kiss::GateConfig::default())),
            ignore: &[],
            timing: true,
            config: Some(Path::new("/tmp/kiss-extra.toml")),
            language_tables: kiss::LanguageTablesPresent::both(),
        }
    }

    fn output(ok: bool, stdout: &str, stderr: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(if ok { 0 } else { 1 << 8 }),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn gather_roots_from_env_parses_properly() {
        let old = std::env::var_os(SHARD_ENV);

        unsafe {
            std::env::remove_var(SHARD_ENV);
        }
        assert_eq!(gather_roots_from_env(), None);

        unsafe {
            std::env::set_var(SHARD_ENV, "   \n  \n");
        }
        assert_eq!(gather_roots_from_env(), None);

        unsafe {
            std::env::set_var(SHARD_ENV, "foo/a.rs\nbar/b.rs\n");
        }
        assert_eq!(
            gather_roots_from_env(),
            Some(vec![PathBuf::from("foo/a.rs"), PathBuf::from("bar/b.rs")])
        );

        match old {
            Some(val) => unsafe { std::env::set_var(SHARD_ENV, val) },
            None => unsafe { std::env::remove_var(SHARD_ENV) },
        }
    }

    #[test]
    fn publish_sharded_outputs_reports_totals_and_status() {
        let py = output(
            true,
            "Analyzed: 1 files, 2 code_units, 3 statements, 4 graph_nodes, 5 graph_edges\nNO VIOLATIONS\n",
            "",
        );
        let rs = output(
            true,
            "Analyzed: 2 files, 3 code_units, 4 statements, 5 graph_nodes, 6 graph_edges\nNO VIOLATIONS\n",
            "",
        );
        assert_eq!(publish_sharded_outputs(&py, &[rs]), 0);

        let py_fail = output(false, "warn: fail\n", "err text\n");
        assert_eq!(publish_sharded_outputs(&py_fail, &[]), 1);
    }

    #[test]
    fn run_split_check_sharded_executes_with_mock_exes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().to_string_lossy().to_string();
        let args = sample_args(&path);

        assert_eq!(run_split_check_sharded(Path::new("/bin/true"), &args), 0);

        for i in 0..65 {
            std::fs::write(tmp.path().join(format!("file_{i}.rs")), "fn f() {}").unwrap();
        }

        assert_eq!(run_split_check_sharded(Path::new("/bin/true"), &args), 0);
        assert_eq!(run_split_check_sharded(Path::new("/bin/false"), &args), 1);
        assert_eq!(
            run_split_check_sharded(Path::new("/no/such/kiss-binary"), &args),
            0
        );
    }
}
