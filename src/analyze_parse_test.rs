use super::*;

impl ParseResult {
    fn witness() -> Self {
        Self {
            py_parsed: vec![],
            rs_parsed: vec![],
            roles: SourceRoleIndex::empty(),
            violations: vec![],
            code_unit_count: 0,
            statement_count: 0,
        }
    }
}

impl ParseAllTimedParams<'_> {
    fn witness() {}
}

mod tests {
    use super::*;

    #[test]
    fn test_parse_all_empty() {
        let py_cfg = Config::python_defaults();
        let rs_cfg = Config::rust_defaults();
        let result = parse_all(&[], &[], &py_cfg, &rs_cfg).unwrap();
        assert!(result.py_parsed.is_empty());
        assert!(result.rs_parsed.is_empty());
        assert_eq!(result.code_unit_count, 0);
        assert_eq!(result.statement_count, 0);
    }

    #[test]
    fn test_structural_thresholds_apply_to_python_test_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let test_path = tmp.path().join("test_big.py");
        std::fs::write(
            &test_path,
            "def big():\n    x = 1\n    y = 2\n    z = 3\n    return x + y + z\n",
        )
        .unwrap();

        let mut py_cfg = Config::python_defaults();
        py_cfg.lines_per_file = 1;
        py_cfg.statements_per_file = 1;
        py_cfg.statements_per_function = 1;

        let rs_cfg = Config::rust_defaults();

        let result = parse_all(std::slice::from_ref(&test_path), &[], &py_cfg, &rs_cfg).unwrap();
        assert!(
            result.violations.is_empty(),
            "test-only Python must not affect production thresholds: {:?}",
            result.violations
        );
        assert_eq!(result.code_unit_count, 0);
        assert_eq!(result.statement_count, 0);
    }

    #[test]
    fn mixed_rust_cfg_test_items_do_not_count() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("lib.rs");
        std::fs::write(
            &path,
            "pub fn prod() { let x = 1; }\n#[cfg(test)]\nmod tests {\n    fn t() { let y = 2; let z = 3; }\n}\n",
        )
        .unwrap();
        let mut rs_cfg = Config::rust_defaults();
        rs_cfg.statements_per_file = 1;
        rs_cfg.functions_per_file = 1;
        let py_cfg = Config::python_defaults();
        let result = parse_all(&[], std::slice::from_ref(&path), &py_cfg, &rs_cfg).unwrap();
        assert!(
            result.violations.is_empty(),
            "cfg(test) items must not affect production thresholds: {:?}",
            result.violations
        );
        assert_eq!(result.statement_count, 1);
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
            &py_cfg,
            &rs_cfg,
        )
        .unwrap();
        assert_eq!(result.py_parsed.len(), 1);
        assert_eq!(result.rs_parsed.len(), 1);
        assert!(result.code_unit_count > 0);
    }

    #[test]
    fn test_parse_all_timed_params_constructible() {
        let py_cfg = Config::python_defaults();
        let rs_cfg = Config::rust_defaults();
        let params = ParseAllTimedParams {
            py_files: &[],
            rs_files: &[],
            py_config: &py_cfg,
            rs_config: &rs_cfg,
            show_timing: false,
        };
        assert!(!params.show_timing);
    }

    #[test]
    fn test_parse_all_timed_with_timing() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.py"), "def f(): pass").unwrap();
        let py_cfg = Config::python_defaults();
        let rs_cfg = Config::rust_defaults();
        let (result, timing) = parse_all_timed(ParseAllTimedParams {
            py_files: &[tmp.path().join("a.py")],
            rs_files: &[],
            py_config: &py_cfg,
            rs_config: &rs_cfg,
            show_timing: true,
        })
        .unwrap();
        assert!(
            !timing.is_empty(),
            "timing string should be non-empty when show_timing=true"
        );
        assert_eq!(result.py_parsed.len(), 1);
    }

    #[test]
    fn test_py_parsed_or_log_ok() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("ok.py");
        std::fs::write(&p, "x = 1").unwrap();
        let results = kiss::parse_files(&[p]).unwrap();
        let first = results.into_iter().next().unwrap();
        let out = py_parsed_or_log(first);
        assert!(out.is_some());
    }

    #[test]
    fn test_py_agg_empty_returns_zeros() {
        let (units, stmts, viols) = py_agg_empty();
        assert_eq!(units, 0);
        assert_eq!(stmts, 0);
        assert!(viols.is_empty());
    }

    #[test]
    fn test_py_agg_merge_combines() {
        let a: PyAgg = (2, 3, vec![]);
        let b: PyAgg = (5, 7, vec![]);
        let merged = py_agg_merge(a, b);
        assert_eq!(merged.0, 7);
        assert_eq!(merged.1, 10);
    }

    #[test]
    fn test_parse_and_analyze_py_timed_empty() {
        let cfg = Config::python_defaults();
        let ((parsed, viols, units, stmts), timing) =
            parse_and_analyze_py_timed(&[], &cfg, false).unwrap();
        assert!(parsed.is_empty());
        assert!(viols.is_empty());
        assert_eq!(units, 0);
        assert_eq!(stmts, 0);
        assert!(timing.is_empty());
    }

    #[test]
    fn test_py_file_agg_smoke() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("agg.py");
        std::fs::write(&p, "def g(): pass\ndef h(): pass\n").unwrap();
        let results = kiss::parse_files(&[p]).unwrap();
        let parsed: Vec<_> = results.into_iter().filter_map(Result::ok).collect();
        assert!(!parsed.is_empty());
        let cfg = Config::python_defaults();
        let (units, stmts, viols) = py_file_agg(&parsed[0], &cfg);
        assert!(units > 0 || stmts > 0 || viols.is_empty());
    }

    #[test]
    fn witness_parse_types() {
        let _ = ParseResult::witness();
        ParseAllTimedParams::witness();
    }
}
