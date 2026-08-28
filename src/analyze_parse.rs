use rayon::prelude::*;
use std::path::PathBuf;

use kiss::code_roles::{
    CodeRole, RoleBuildError, SourceRoleIndex, build_source_role_index, is_test_only_file,
};
use kiss::counts::analyze_file_with_statement_count;
use kiss::units::count_code_units;
use kiss::{
    Config, ParsedFile, ParsedRustFile, Violation, analyze_rust_file_include_rollup_with_roles,
    analyze_rust_file_with_roles, compute_rust_file_metrics_with_roles, extract_rust_code_units,
    parse_files, parse_rust_files,
};

pub struct ParseResult {
    pub py_parsed: Vec<ParsedFile>,
    pub rs_parsed: Vec<ParsedRustFile>,
    pub roles: SourceRoleIndex,
    pub violations: Vec<Violation>,
    pub code_unit_count: usize,
    pub statement_count: usize,
}

pub struct ParseAllTimedParams<'a> {
    pub py_files: &'a [PathBuf],
    pub rs_files: &'a [PathBuf],
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
    pub show_timing: bool,
}

#[allow(dead_code)]
pub fn parse_all(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
) -> Result<ParseResult, RoleBuildError> {
    parse_all_timed(ParseAllTimedParams {
        py_files,
        rs_files,
        py_config,
        rs_config,
        show_timing: false,
    })
    .map(|(result, _)| result)
}

pub fn parse_all_timed(
    p: ParseAllTimedParams<'_>,
) -> Result<(ParseResult, String), RoleBuildError> {
    let ((py_parsed, py_timing), rs_parsed) = parse_sources(p.py_files, p.rs_files, p.show_timing)?;
    let t_roles = std::time::Instant::now();
    let roles = build_source_role_index(&py_parsed, &rs_parsed, p.py_files, p.rs_files)?;
    log_phase_timing(p.show_timing, "roles", t_roles);
    let t_an = std::time::Instant::now();
    let (py_agg, rs_agg) = std::thread::scope(|scope| {
        let py_handle = scope.spawn(|| analyze_py_parsed(&py_parsed, p.py_config, &roles));
        let rs_agg = analyze_rs_parsed(&rs_parsed, p.rs_config, &roles);
        (py_handle.join().expect("python analyze thread"), rs_agg)
    });
    log_phase_timing(p.show_timing, "analyze", t_an);
    let (py_units, py_stmts, mut viols) = py_agg;
    let (rs_units, rs_stmts, rs_viols) = rs_agg?;
    viols.extend(rs_viols);
    let viols = drop_test_only_violations(viols, &roles);
    Ok((
        ParseResult {
            py_parsed,
            rs_parsed,
            roles,
            violations: viols,
            code_unit_count: py_units + rs_units,
            statement_count: py_stmts + rs_stmts,
        },
        py_timing,
    ))
}

type ParseSourcesOut = ((Vec<ParsedFile>, String), Vec<ParsedRustFile>);

fn parse_sources(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    show_timing: bool,
) -> Result<ParseSourcesOut, RoleBuildError> {
    let (py_parsed, rs_parsed, py_secs, rs_secs) = parse_py_and_rs(py_files, rs_files)?;
    let timing = if show_timing {
        format!("py: parse={py_secs:.2}s, rs: parse={rs_secs:.2}s")
    } else {
        String::new()
    };
    Ok(((py_parsed, timing), rs_parsed))
}

fn parse_py_and_rs(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
) -> Result<(Vec<ParsedFile>, Vec<ParsedRustFile>, f64, f64), RoleBuildError> {
    if py_files.is_empty() {
        let t = std::time::Instant::now();
        let rs_parsed = parse_rs_files(rs_files)?;
        return Ok((Vec::new(), rs_parsed, 0.0, t.elapsed().as_secs_f64()));
    }
    if rs_files.is_empty() {
        let t = std::time::Instant::now();
        let py_parsed = parse_py_files(py_files)?;
        return Ok((py_parsed, Vec::new(), t.elapsed().as_secs_f64(), 0.0));
    }
    let (py_out, rs_out) = std::thread::scope(|scope| {
        let py_handle = scope.spawn(|| {
            let t = std::time::Instant::now();
            let parsed = parse_py_files(py_files);
            (parsed, t.elapsed().as_secs_f64())
        });
        let t = std::time::Instant::now();
        let rs_parsed = parse_rs_files(rs_files);
        let rs_secs = t.elapsed().as_secs_f64();
        let (py_parsed, py_secs) = py_handle.join().expect("python parse");
        ((py_parsed, py_secs), (rs_parsed, rs_secs))
    });
    Ok((py_out.0?, rs_out.0?, py_out.1, rs_out.1))
}

pub(crate) fn parse_py_files(files: &[PathBuf]) -> Result<Vec<ParsedFile>, RoleBuildError> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let results = parse_files(files).map_err(|err| RoleBuildError::PythonParse {
        path: files[0].clone(),
        message: err.to_string(),
    })?;
    let mut parsed = Vec::new();
    for (path, result) in files.iter().zip(results) {
        match result {
            Ok(file) => parsed.push(file),
            Err(err) => {
                return Err(RoleBuildError::PythonParse {
                    path: path.clone(),
                    message: err.to_string(),
                });
            }
        }
    }
    Ok(parsed)
}

pub(crate) fn parse_py_files_pooled(files: &[PathBuf]) -> Result<Vec<ParsedFile>, RoleBuildError> {
    let n = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(8)
        .clamp(1, 8);
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build()
        .map_err(|err| RoleBuildError::PythonParse {
            path: files.first().cloned().unwrap_or_default(),
            message: err.to_string(),
        })?
        .install(|| parse_py_files(files))
}

pub(crate) fn parse_rs_files(files: &[PathBuf]) -> Result<Vec<ParsedRustFile>, RoleBuildError> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let mut parsed = Vec::new();
    for (path, result) in files.iter().zip(parse_rust_files(files)) {
        match result {
            Ok(p) => parsed.push(p),
            Err(err) => {
                return Err(RoleBuildError::RustParse {
                    path: path.clone(),
                    message: err.to_string(),
                });
            }
        }
    }
    Ok(parsed)
}

pub(crate) fn parse_classified(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
) -> Result<(Vec<ParsedFile>, Vec<ParsedRustFile>, SourceRoleIndex), RoleBuildError> {
    let (py_parsed, rs_parsed, _, _) = parse_py_and_rs(py_files, rs_files)?;
    let roles = build_source_role_index(&py_parsed, &rs_parsed, py_files, rs_files)?;
    Ok((py_parsed, rs_parsed, roles))
}

pub(crate) fn analyze_py_parsed(
    parsed: &[ParsedFile],
    config: &Config,
    roles: &SourceRoleIndex,
) -> PyAgg {
    parsed
        .par_iter()
        .filter(|p| !is_test_only_file(roles, &p.path))
        .map(|p| py_file_agg(p, config))
        .reduce(py_agg_empty, py_agg_merge)
}

pub(crate) fn analyze_rs_parsed(
    parsed: &[ParsedRustFile],
    config: &Config,
    roles: &SourceRoleIndex,
) -> Result<(usize, usize, Vec<Violation>), RoleBuildError> {
    let mut unit_count = 0;
    let mut stmt_count = 0;
    let mut viols = Vec::new();
    for p in parsed {
        if is_test_only_file(roles, &p.path) {
            continue;
        }
        unit_count += extract_rust_code_units(p).len();
        stmt_count += compute_rust_file_metrics_with_roles(p, Some(roles)).statements;
        viols.extend(analyze_rust_file_with_roles(p, config, Some(roles)));
    }
    let refs: Vec<&ParsedRustFile> = parsed.iter().collect();
    let include_graph = kiss::rust_graph::build_include_graph(&refs);
    let by_path: std::collections::HashMap<_, _> = parsed
        .iter()
        .map(|p| (kiss::rust_include::canonical_path(&p.path), p))
        .collect();
    for parent in parsed {
        if is_test_only_file(roles, &parent.path) {
            continue;
        }
        let included_paths = include_graph.transitive_from(&parent.path);
        if included_paths.is_empty() {
            continue;
        }
        let included: Vec<&ParsedRustFile> = included_paths
            .iter()
            .filter_map(|path| by_path.get(path).copied())
            .collect();
        viols.extend(analyze_rust_file_include_rollup_with_roles(
            parent,
            &included,
            config,
            Some(roles),
        ));
    }
    Ok((unit_count, stmt_count, viols))
}

fn log_phase_timing(show: bool, label: &str, started: std::time::Instant) {
    if show {
        eprintln!("[TIMING] {label}={:.2}s", started.elapsed().as_secs_f64());
    }
}

fn drop_test_only_violations(viols: Vec<Violation>, roles: &SourceRoleIndex) -> Vec<Violation> {
    viols
        .into_iter()
        .filter(|v| {
            !is_test_only_file(roles, &v.file)
                && roles.role_for_span(
                    &v.file,
                    kiss::code_roles::SourceSpan::new(
                        kiss::code_roles::SourcePosition::new(v.line, 0),
                        kiss::code_roles::SourcePosition::new(v.line, 1),
                    ),
                ) != CodeRole::TestOnly
        })
        .collect()
}

type PyAgg = (usize, usize, Vec<Violation>);

#[cfg(test)]
pub fn py_parsed_or_log(r: Result<ParsedFile, kiss::ParseError>) -> Option<ParsedFile> {
    match r {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("Error parsing Python: {e}");
            None
        }
    }
}

fn py_file_agg(p: &ParsedFile, config: &Config) -> PyAgg {
    let units = count_code_units(p);
    let (stmts, viols) = analyze_file_with_statement_count(p, config);
    (units, stmts, viols)
}

const fn py_agg_empty() -> PyAgg {
    (0, 0, Vec::new())
}

fn py_agg_merge(mut a: PyAgg, b: PyAgg) -> PyAgg {
    a.0 += b.0;
    a.1 += b.1;
    a.2.extend(b.2);
    a
}

#[cfg(test)]
type PyAnalyzeTimed = ((Vec<ParsedFile>, Vec<Violation>, usize, usize), String);

#[cfg(test)]
fn parse_and_analyze_py_timed(
    files: &[PathBuf],
    config: &Config,
    show_timing: bool,
) -> Result<PyAnalyzeTimed, RoleBuildError> {
    let t0 = std::time::Instant::now();
    let parsed = parse_py_files(files)?;
    let t1 = std::time::Instant::now();
    let (unit_count, stmt_count, viols) = parsed
        .par_iter()
        .map(|p| py_file_agg(p, config))
        .reduce(py_agg_empty, py_agg_merge);
    let t2 = std::time::Instant::now();
    let timing = if show_timing {
        format!(
            "py: parse={:.2}s, analyze={:.2}s",
            t1.duration_since(t0).as_secs_f64(),
            t2.duration_since(t1).as_secs_f64()
        )
    } else {
        String::new()
    };
    Ok(((parsed, viols, unit_count, stmt_count), timing))
}

#[cfg(test)]
pub fn parse_and_analyze_rs(
    files: &[PathBuf],
    config: &Config,
) -> Result<(Vec<ParsedRustFile>, Vec<Violation>, usize, usize), RoleBuildError> {
    let parsed = parse_rs_files(files)?;
    let roles = build_source_role_index(&[], &parsed, &[], files)?;
    let (units, stmts, viols) = analyze_rs_parsed(&parsed, config, &roles)?;
    Ok((parsed, viols, units, stmts))
}

#[cfg(test)]
#[path = "analyze_parse_test.rs"]
mod analyze_parse_test;
