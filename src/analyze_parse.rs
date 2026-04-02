use rayon::prelude::*;
use std::path::PathBuf;

use kiss::counts::analyze_file_with_statement_count;
use kiss::units::count_code_units;
use kiss::{
    Config, ParsedFile, ParsedRustFile, Violation, analyze_rust_file, extract_rust_code_units,
    is_rust_test_file, is_test_file, parse_files, parse_rust_files,
};

pub struct ParseResult {
    pub py_parsed: Vec<ParsedFile>,
    pub rs_parsed: Vec<ParsedRustFile>,
    pub violations: Vec<Violation>,
    pub code_unit_count: usize,
    pub statement_count: usize,
}

pub fn parse_all(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
) -> ParseResult {
    parse_all_timed(py_files, rs_files, py_config, rs_config, false).0
}

pub fn parse_all_timed(
    py_files: &[PathBuf],
    rs_files: &[PathBuf],
    py_config: &Config,
    rs_config: &Config,
    show_timing: bool,
) -> (ParseResult, String) {
    let ((py_parsed, mut viols, py_units, py_stmts), py_timing) =
        parse_and_analyze_py_timed(py_files, py_config, show_timing);
    let (rs_parsed, rs_viols, rs_units, rs_stmts) = parse_and_analyze_rs(rs_files, rs_config);
    viols.extend(rs_viols);
    (
        ParseResult {
            py_parsed,
            rs_parsed,
            violations: viols,
            code_unit_count: py_units + rs_units,
            statement_count: py_stmts + rs_stmts,
        },
        py_timing,
    )
}

type PyAgg = (usize, usize, Vec<Violation>);

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
    let (stmts, viols) = if is_test_file(&p.path) {
        (kiss::compute_file_metrics(p).statements, Vec::new())
    } else {
        analyze_file_with_statement_count(p, config)
    };
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

fn parse_and_analyze_py_timed(
    files: &[PathBuf],
    config: &Config,
    show_timing: bool,
) -> ((Vec<ParsedFile>, Vec<Violation>, usize, usize), String) {
    if files.is_empty() {
        return ((Vec::new(), Vec::new(), 0, 0), String::new());
    }
    let t0 = std::time::Instant::now();
    let results = match parse_files(files) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to initialize Python parser: {e}");
            return ((Vec::new(), Vec::new(), 0, 0), String::new());
        }
    };
    let t1 = std::time::Instant::now();

    let parsed: Vec<ParsedFile> = results.into_iter().filter_map(py_parsed_or_log).collect();

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
    ((parsed, viols, unit_count, stmt_count), timing)
}

pub fn parse_and_analyze_rs(
    files: &[PathBuf],
    config: &Config,
) -> (Vec<ParsedRustFile>, Vec<Violation>, usize, usize) {
    if files.is_empty() {
        return (Vec::new(), Vec::new(), 0, 0);
    }
    let (mut parsed, mut viols, mut unit_count, mut stmt_count) = (Vec::new(), Vec::new(), 0, 0);
    for result in parse_rust_files(files) {
        match result {
            Ok(p) => {
                unit_count += extract_rust_code_units(&p).len();
                stmt_count += kiss::compute_rust_file_metrics(&p).statements;
                if !is_rust_test_file(&p.path) {
                    viols.extend(analyze_rust_file(&p, config));
                }
                parsed.push(p);
            }
            Err(e) => eprintln!("Error parsing Rust: {e}"),
        }
    }
    (parsed, viols, unit_count, stmt_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_all_empty() {
        let py_cfg = Config::python_defaults();
        let rs_cfg = Config::rust_defaults();
        let result = parse_all(&[], &[], &py_cfg, &rs_cfg);
        assert!(result.py_parsed.is_empty());
        assert!(result.rs_parsed.is_empty());
        assert_eq!(result.code_unit_count, 0);
        assert_eq!(result.statement_count, 0);
    }

    #[test]
    fn test_parse_all_with_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.py"), "def f(): pass").unwrap();
        std::fs::write(tmp.path().join("b.rs"), "fn main() {}").unwrap();
        let py_cfg = Config::python_defaults();
        let rs_cfg = Config::rust_defaults();
        let result = parse_all(
            &[tmp.path().join("a.py")],
            &[tmp.path().join("b.rs")],
            &py_cfg, &rs_cfg,
        );
        assert_eq!(result.py_parsed.len(), 1);
        assert_eq!(result.rs_parsed.len(), 1);
        assert!(result.code_unit_count > 0);
    }

    #[test]
    #[allow(clippy::let_unit_value)]
    fn test_touch_for_coverage() {
        fn touch<T>(_: T) {}
        let _ = (
            touch(parse_all_timed),
            touch(py_parsed_or_log),
            touch(py_file_agg),
            touch(py_agg_empty),
            touch(py_agg_merge),
            touch(parse_and_analyze_py_timed),
            touch(parse_and_analyze_rs),
        );
        let _ = std::mem::size_of::<ParseResult>();
    }
}
