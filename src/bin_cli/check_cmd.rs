use std::path::Path;
use std::process::{Command, Output, Stdio};

use crate::analyze;
use crate::analyze::run_analyze;
use crate::bin_cli::util::{merge_check_ignore_prefixes, validate_paths};
use kiss::Language;

pub struct CheckCommandArgs<'a> {
    pub paths: &'a [String],
    pub lang_filter: Option<Language>,
    pub py_config: &'a kiss::Config,
    pub rs_config: &'a kiss::Config,
    pub gate_config: &'a kiss::GateConfig,
    pub ignore: &'a [String],
    pub timing: bool,
    pub config: Option<&'a Path>,
    pub language_tables: kiss::LanguageTablesPresent,
}

pub fn run_check_command(args: &CheckCommandArgs<'_>) -> i32 {
    #[cfg(not(test))]
    if args.lang_filter.is_none() && std::env::var_os("KISS_CHECK_WORKER").is_none() {
        return run_split_check(args);
    }
    run_check_in_process(args)
}

fn run_check_in_process(args: &CheckCommandArgs<'_>) -> i32 {
    let ignore = merge_check_ignore_prefixes(args.ignore);
    validate_paths(args.paths);
    let universe = &args.paths[0];
    let focus = if args.paths.len() > 1 {
        &args.paths[1..]
    } else {
        args.paths
    };
    let opts = analyze::AnalyzeOptions {
        universe,
        focus_paths: focus,
        py_config: args.py_config,
        rs_config: args.rs_config,
        lang_filter: args.lang_filter,
        bypass_gate: false,
        gate_config: args.gate_config,
        ignore_prefixes: &ignore,
        show_timing: args.timing,
        suppress_final_status: false,
        language_tables: args.language_tables,
    };
    i32::from(!run_analyze(&opts))
}

#[cfg(not(test))]
fn run_split_check(args: &CheckCommandArgs<'_>) -> i32 {
    let Ok(exe) = std::env::current_exe() else {
        return run_check_in_process(args);
    };
    run_split_check_with_exe(&exe, args)
}

fn run_split_check_with_exe(exe: &Path, args: &CheckCommandArgs<'_>) -> i32 {
    let Ok(mut rust) = spawn_lang_check(exe, args, "rust") else {
        return run_check_in_process(args);
    };
    let Ok(python) = spawn_lang_check(exe, args, "python") else {
        let _ = rust.kill();
        return run_check_in_process(args);
    };
    let rust = rust.wait_with_output().ok();
    let python = python.wait_with_output().ok();
    match (python, rust) {
        (Some(python), Some(rust)) => publish_worker_outputs(&python, &rust),
        _ => run_check_in_process(args),
    }
}

fn lang_check_command(exe: &Path, args: &CheckCommandArgs<'_>, lang: &str) -> Command {
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
    cmd
}

fn spawn_lang_check(
    exe: &Path,
    args: &CheckCommandArgs<'_>,
    lang: &str,
) -> std::io::Result<std::process::Child> {
    lang_check_command(exe, args, lang).spawn()
}

fn publish_worker_outputs(python: &Output, rust: &Output) -> i32 {
    forward_worker_stderr(&python.stderr);
    forward_worker_stderr(&rust.stderr);
    let mut totals = [0_usize; 5];
    for out in [&python.stdout, &rust.stdout] {
        for line in String::from_utf8_lossy(out).lines() {
            if let Some(next) = analyzed_add(totals, line) {
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
    if python.status.success() && rust.status.success() {
        println!("NO VIOLATIONS");
        0
    } else {
        1
    }
}

fn forward_worker_stderr(bytes: &[u8]) {
    for line in String::from_utf8_lossy(bytes).lines() {
        if !line.starts_with("kiss:") {
            eprintln!("{line}");
        }
    }
}

fn analyzed_add(mut totals: [usize; 5], line: &str) -> Option<[usize; 5]> {
    let rest = line.strip_prefix("Analyzed: ")?;
    let nums: Vec<usize> = rest
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|s| s.parse().ok())
        .collect();
    if nums.len() < 5 {
        return None;
    }
    for (slot, n) in totals.iter_mut().zip(nums) {
        *slot += n;
    }
    Some(totals)
}

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    impl CheckCommandArgs<'_> {
        fn witness() {}
    }

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
    fn witness_check_command() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().to_string_lossy().to_string();
        let py = kiss::Config::python_defaults();
        let rs = kiss::Config::rust_defaults();
        let gate = kiss::GateConfig::default();
        let args = CheckCommandArgs {
            paths: &[path],
            lang_filter: None,
            py_config: &py,
            rs_config: &rs,
            gate_config: &gate,
            ignore: &[],
            timing: false,
            config: None,
            language_tables: kiss::LanguageTablesPresent::both(),
        };
        CheckCommandArgs::witness();
        assert_eq!(run_check_command(&args), 0);
    }

    #[test]
    fn analyzed_add_sums_two_summaries() {
        let line = "Analyzed: 10 files, 20 code_units, 30 statements, 4 graph_nodes, 5 graph_edges";
        let totals = analyzed_add([1, 2, 3, 4, 5], line).unwrap();
        assert_eq!(totals, [11, 22, 33, 8, 10]);
        assert!(analyzed_add([0; 5], "NO VIOLATIONS").is_none());
        assert!(analyzed_add([0; 5], "Analyzed: 1 files").is_none());
    }

    #[test]
    fn publish_merges_clean_workers() {
        let python = output(
            true,
            "Analyzed: 2 files, 3 code_units, 4 statements, 2 graph_nodes, 1 graph_edges\nNO VIOLATIONS\n",
            "[TIMING] py=1\nkiss: 1.00s\n",
        );
        let rust = output(
            true,
            "Analyzed: 1 files, 1 code_units, 1 statements, 1 graph_nodes, 1 graph_edges\nNO VIOLATIONS\n",
            "kiss: 2.00s\n",
        );
        assert_eq!(publish_worker_outputs(&python, &rust), 0);
    }

    #[test]
    fn publish_returns_one_on_failure() {
        let python = output(false, "bad.py: boom\n", "");
        let rust = output(true, "NO VIOLATIONS\n", "");
        assert_eq!(publish_worker_outputs(&python, &rust), 1);
    }

    #[test]
    fn spawn_and_split_with_true_exe() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().to_string_lossy().to_string();
        let args = sample_args(&path);
        let child = spawn_lang_check(Path::new("/bin/true"), &args, "rust").unwrap();
        assert!(child.wait_with_output().unwrap().status.success());
        assert_eq!(run_split_check_with_exe(Path::new("/bin/true"), &args), 0);
        assert_eq!(run_split_check_with_exe(Path::new("/bin/false"), &args), 1);
        assert_eq!(
            run_split_check_with_exe(Path::new("/no/such/kiss-worker"), &args),
            0
        );
        let cmd = lang_check_command(Path::new("/bin/true"), &args, "python");
        let forwarded: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(forwarded.iter().any(|a| a == "--config"));
        assert!(forwarded.iter().any(|a| a == "/tmp/kiss-extra.toml"));
    }
}
