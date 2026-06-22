use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use crate::test_discovery::{self, args as disc_args};
use kiss::test_refs::{is_in_test_directory, is_test_file};
use kiss::{parse_files, parse_rust_files, rust_test_functions_in, test_functions_in};
use rpytest_runner::subprocess_pytest_runner;
use rslip::{CacheStatus, Rslip, RslipError, RslipOutcome, RslipRequest};

pub const NO_COVERING_TESTS_MSG: &str = "NO COVERING TESTS";

pub fn py_selector(test_path: &Path, test_id: &str) -> String {
    format!("{}::{}", test_path.display(), test_id)
}

pub fn merge_exit_codes(a: i32, b: i32) -> i32 {
    a.max(b)
}

pub fn collect_selectors_from_defs(
    defs: &[test_discovery::DefEntry],
) -> BTreeSet<(PathBuf, String)> {
    let mut set = BTreeSet::new();
    for (_src, _name, _line, cov) in defs {
        if let Some(tests) = cov {
            for (tp, tid) in tests {
                set.insert((tp.clone(), tid.clone()));
            }
        }
    }
    set
}

pub fn partition_changed_paths(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut source = Vec::new();
    let mut test = Vec::new();
    for p in paths {
        let is_py = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("py"));
        let is_rs = kiss::Language::is_rust_path(p);
        if is_py {
            if is_test_file(p) || is_in_test_directory(p) {
                test.push(p.clone());
            } else {
                source.push(p.clone());
            }
        } else if is_rs {
            if is_in_test_directory(p) {
                test.push(p.clone());
            } else {
                source.push(p.clone());
            }
        }
    }
    (source, test)
}

pub fn enumerate_tests_in_changed_files(
    test_paths: &[PathBuf],
) -> Result<BTreeSet<(PathBuf, String)>, String> {
    let mut out = BTreeSet::new();
    let py: Vec<_> = test_paths
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("py")))
        .cloned()
        .collect();
    let rs: Vec<_> = test_paths
        .iter()
        .filter(|p| kiss::Language::is_rust_path(p))
        .cloned()
        .collect();
    if !py.is_empty() {
        let parsed = parse_files(&py).map_err(|e| e.to_string())?;
        for (path, r) in py.iter().zip(parsed) {
            let pf = r.map_err(|e| {
                format!("error: kiss test: failed to parse {}: {e}", path.display())
            })?;
            let ids = test_functions_in(&pf);
            for id in ids {
                out.insert((pf.path.clone(), id));
            }
        }
    }
    if !rs.is_empty() {
        let parsed = parse_rust_files(&rs);
        for (path, r) in rs.iter().zip(parsed) {
            let pf = r.map_err(|e| {
                format!("error: kiss test: failed to parse {}: {e}", path.display())
            })?;
            let ids = rust_test_functions_in(&pf);
            for id in ids {
                out.insert((pf.path.clone(), id));
            }
        }
    }
    Ok(out)
}

pub fn shell_quote_line(argv: &[String]) -> String {
    argv.iter()
        .map(|a| shlex_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn shlex_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '=' | ','))
        && !s.starts_with('-')
    {
        return s.to_string();
    }
    let mut out = String::from('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\"'\"'");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

pub fn build_pytest_argv(selectors: &[String], extra: &[String]) -> Vec<String> {
    let mut v = vec!["python".into(), "-m".into(), "pytest".into()];
    v.extend(selectors.iter().cloned());
    v.extend(extra.iter().cloned());
    v
}

pub fn run_rslip_selectors(
    repo_root: &Path,
    selectors: &[String],
    extra: &[String],
    force_rerun: bool,
    jobs: usize,
) -> Result<i32, String> {
    assert!(jobs > 0, "jobs must be greater than zero");
    let (python_version, pytest_version) = detect_rslip_versions(repo_root)?;
    let reqs: Vec<_> = selectors
        .iter()
        .map(|selector| {
            rslip_request_from_parts(
                repo_root,
                selector,
                extra,
                &python_version,
                &pytest_version,
                force_rerun,
            )
        })
        .collect::<Result<_, _>>()?;
    let mut code = 0;
    for result in run_rslip_requests_bounded(reqs, jobs) {
        let outcome = result.map_err(format_rslip_error)?;
        print_rslip_outcome(&outcome);
        if outcome.status == rpytest_runner::TestStatus::Failed {
            code = merge_exit_codes(code, outcome.exit_code.unwrap_or(1));
        }
    }
    Ok(code)
}

pub fn rslip_request_from_parts(
    repo_root: &Path,
    selector: &str,
    extra: &[String],
    python_version: &str,
    pytest_version: &str,
    force_rerun: bool,
) -> Result<RslipRequest, String> {
    if !python_version_supports_rslip(python_version) {
        return Err(format!(
            "error: kiss test: rslip requires Python 3.12+, found {python_version}"
        ));
    }
    Ok(RslipRequest {
        nodeid: selector.to_string(),
        cwd: repo_root.to_path_buf(),
        source_root: repo_root.to_path_buf(),
        python: PathBuf::from("python"),
        python_version: python_version.to_string(),
        pytest_version: pytest_version.to_string(),
        pytest_args: extra.to_vec(),
        env: BTreeMap::new(),
        cache_root: repo_root.join(".kiss").join("rslip_cache"),
        force_rerun,
    })
}

fn detect_rslip_versions(repo_root: &Path) -> Result<(String, String), String> {
    let python = PathBuf::from("python");
    let python_version = command_stdout(
        &python,
        &[
            "-c",
            "import sys; print('.'.join(map(str, sys.version_info[:3])))",
        ],
        repo_root,
    )?;
    let pytest_version = command_stdout(
        &python,
        &["-c", "import pytest; print(pytest.__version__)"],
        repo_root,
    )?;
    Ok((python_version, pytest_version))
}

fn python_version_supports_rslip(version: &str) -> bool {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
    matches!((major, minor), (Some(major), Some(minor)) if major > 3 || (major == 3 && minor >= 12))
}

fn run_rslip_requests_bounded(
    reqs: Vec<RslipRequest>,
    jobs: usize,
) -> Vec<Result<RslipOutcome, RslipError>> {
    assert!(jobs > 0, "jobs must be greater than zero");
    let len = reqs.len();
    let mut out = Vec::new();
    out.resize_with(len, || {
        Err(RslipError::InvalidRequest(
            "rslip worker did not report a result".to_string(),
        ))
    });
    if len == 0 {
        return out;
    }

    let (tx, rx) = mpsc::channel();
    let mut indexed_reqs = reqs.into_iter().enumerate();
    let mut running = 0usize;
    for _ in 0..jobs.min(len) {
        if let Some((index, req)) = indexed_reqs.next() {
            spawn_rslip_job(index, req, tx.clone());
            running += 1;
        }
    }

    while running > 0 {
        let Ok((index, result)) = rx.recv() else {
            break;
        };
        running -= 1;
        out[index] = result;
        if let Some((next_index, next_req)) = indexed_reqs.next() {
            spawn_rslip_job(next_index, next_req, tx.clone());
            running += 1;
        }
    }
    out
}

fn spawn_rslip_job(
    index: usize,
    req: RslipRequest,
    tx: mpsc::Sender<(usize, Result<RslipOutcome, RslipError>)>,
) {
    thread::spawn(move || {
        let rslip = Rslip::new(subprocess_pytest_runner());
        let result = rslip.run_or_reuse(req);
        let _ = tx.send((index, result));
    });
}

fn print_rslip_outcome(outcome: &RslipOutcome) {
    match (outcome.status, outcome.cache_status) {
        (rpytest_runner::TestStatus::Passed, CacheStatus::Hit) => {
            println!("PASSED (cached): {}", outcome.nodeid);
        }
        (rpytest_runner::TestStatus::Passed, CacheStatus::MissStored) => {
            println!("PASSED: {}", outcome.nodeid);
        }
        (rpytest_runner::TestStatus::Failed, CacheStatus::Hit) => {
            println!("FAILED (cached): {}", outcome.nodeid);
            eprintln!(
                "Failure output was not cached. Re-run with --force to reproduce stdout/stderr."
            );
        }
        (rpytest_runner::TestStatus::Failed, CacheStatus::MissStored) => {
            println!("FAILED: {}", outcome.nodeid);
            if let Some(stderr) = &outcome.stderr
                && !stderr.is_empty()
            {
                eprint!("{}", String::from_utf8_lossy(stderr));
            }
        }
    }
}

fn format_rslip_error(err: RslipError) -> String {
    format!("error: kiss test: rslip failed: {err:?}")
}

fn command_stdout(program: &Path, args: &[&str], cwd: &Path) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("failed to spawn {}: {e}", program.display()))?;
    if !output.status.success() {
        return Err(format!(
            "error: kiss test: {} failed: {}",
            program.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn build_cargo_test_argv(selectors: &[String], extra: &[String]) -> Vec<String> {
    let mut v = vec!["cargo".into(), "test".into(), "--".into()];
    v.extend(selectors.iter().cloned());
    v.extend(extra.iter().cloned());
    v
}

pub fn run_command_inherit(argv: &[String], cwd: &Path) -> Result<i32, String> {
    if argv.is_empty() {
        return Ok(0);
    }
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).current_dir(cwd);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    let st = cmd
        .status()
        .map_err(|e| format!("failed to spawn {}: {e}", argv[0]))?;
    Ok(st.code().unwrap_or_else(|| i32::from(!st.success())))
}

pub fn discover_for_paths(
    repo_root: &Path,
    source_paths: &[PathBuf],
    lang_filter: Option<kiss::Language>,
    ignore: &[String],
) -> Result<Vec<test_discovery::DefEntry>, String> {
    let path_strs: Vec<String> = source_paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    test_discovery::discover_covering_tests(disc_args::DiscoverArgs {
        universe: &repo_root.to_string_lossy(),
        paths: &path_strs,
        lang_filter,
        ignore,
    })
}

pub fn combined_selectors(
    repo_root: &Path,
    source_paths: &[PathBuf],
    test_paths: &[PathBuf],
    lang_filter: Option<kiss::Language>,
    ignore: &[String],
) -> Result<(Vec<String>, Vec<String>), String> {
    let defs = if source_paths.is_empty() {
        Vec::new()
    } else {
        discover_for_paths(repo_root, source_paths, lang_filter, ignore)?
    };
    let mut py_sel = BTreeSet::new();
    let mut rs_sel = BTreeSet::new();
    for (tp, tid) in collect_selectors_from_defs(&defs) {
        if tp.extension().is_some_and(|e| e.eq_ignore_ascii_case("py")) {
            py_sel.insert(py_selector(&tp, &tid));
        } else if kiss::Language::is_rust_path(&tp) {
            rs_sel.insert(tid);
        }
    }
    for (tp, tid) in enumerate_tests_in_changed_files(test_paths)? {
        if tp.extension().is_some_and(|e| e.eq_ignore_ascii_case("py")) {
            py_sel.insert(py_selector(&tp, &tid));
        } else if kiss::Language::is_rust_path(&tp) {
            rs_sel.insert(tid);
        }
    }
    Ok((py_sel.into_iter().collect(), rs_sel.into_iter().collect()))
}
