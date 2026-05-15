use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use kiss::test_refs::{is_in_test_directory, is_test_file};
use kiss::{parse_files, parse_rust_files, rust_test_functions_in, test_functions_in};
use crate::test_discovery::{self, args as disc_args};

pub const NO_COVERING_TESTS_MSG: &str = "NO COVERING TESTS";

pub fn py_selector(test_path: &Path, test_id: &str) -> String {
    format!("{}::{}", test_path.display(), test_id)
}

pub fn merge_exit_codes(a: i32, b: i32) -> i32 {
    a.max(b)
}

pub fn collect_selectors_from_defs(defs: &[test_discovery::DefEntry]) -> BTreeSet<(PathBuf, String)> {
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
        let is_rs = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rs"));
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

pub fn enumerate_tests_in_changed_files(test_paths: &[PathBuf]) -> Result<BTreeSet<(PathBuf, String)>, String> {
    let mut out = BTreeSet::new();
    let py: Vec<_> = test_paths
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("py")))
        .cloned()
        .collect();
    let rs: Vec<_> = test_paths
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rs")))
        .cloned()
        .collect();
    if !py.is_empty() {
        let parsed = parse_files(&py).map_err(|e| e.to_string())?;
        for (path, r) in py.iter().zip(parsed) {
            let pf = r.map_err(|e| format!("error: kiss test: failed to parse {}: {e}", path.display()))?;
            let ids = test_functions_in(&pf);
            for id in ids {
                out.insert((pf.path.clone(), id));
            }
        }
    }
    if !rs.is_empty() {
        let parsed = parse_rust_files(&rs);
        for (path, r) in rs.iter().zip(parsed) {
            let pf = r.map_err(|e| format!("error: kiss test: failed to parse {}: {e}", path.display()))?;
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
    if s.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '=' | ',')
    }) && !s.starts_with('-')
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
    let mut v = vec![
        "python".into(),
        "-m".into(),
        "pytest".into(),
    ];
    v.extend(selectors.iter().cloned());
    v.extend(extra.iter().cloned());
    v
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
    Ok(st
        .code()
        .unwrap_or_else(|| i32::from(!st.success())))
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
        } else if tp.extension().is_some_and(|e| e.eq_ignore_ascii_case("rs")) {
            rs_sel.insert(tid);
        }
    }
    for (tp, tid) in enumerate_tests_in_changed_files(test_paths)? {
        if tp.extension().is_some_and(|e| e.eq_ignore_ascii_case("py")) {
            py_sel.insert(py_selector(&tp, &tid));
        } else if tp.extension().is_some_and(|e| e.eq_ignore_ascii_case("rs")) {
            rs_sel.insert(tid);
        }
    }
    Ok((
        py_sel.into_iter().collect(),
        rs_sel.into_iter().collect(),
    ))
}
