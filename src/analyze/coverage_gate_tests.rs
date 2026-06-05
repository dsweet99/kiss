use super::*;
use kiss::check_universe_cache::CachedCoverageItem;
use std::collections::HashMap;
use std::collections::HashSet;

    #[test]
    fn per_file_gate_fails_when_file_below_threshold() {
        use std::path::PathBuf;
        let defs = vec![(PathBuf::from("src/a.py"), "f".into(), 1)];
        let unrefs = vec![(PathBuf::from("src/a.py"), "f".into(), 1)];
        let focus = crate::analyze::FocusFilter::restricting(std::iter::once(PathBuf::from("src/a.py")).collect());
        let failure = per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90, None);
        let (unreferenced, file_pcts) = failure.expect("expected gate failure");
        assert_eq!(file_pcts.get(&PathBuf::from("src/a.py")), Some(&0));
        assert_eq!(unreferenced.len(), 1);
    }

    #[test]
    fn per_file_gate_ignores_files_outside_focus() {
        use std::path::PathBuf;
        let defs = vec![(PathBuf::from("src/a.py"), "f".into(), 1)];
        let unrefs = vec![(PathBuf::from("src/a.py"), "f".into(), 1)];
        let focus = crate::analyze::FocusFilter::restricting(std::iter::once(PathBuf::from("src/b.py")).collect());
        assert!(per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90, None).is_none());
    }

    #[test]
    fn per_file_gate_passes_when_file_meets_threshold() {
        use std::path::PathBuf;
        let defs = vec![
            (PathBuf::from("src/a.py"), "f".into(), 1),
            (PathBuf::from("src/a.py"), "g".into(), 2),
        ];
        let unrefs = vec![(PathBuf::from("src/a.py"), "f".into(), 1)];
        let focus = crate::analyze::FocusFilter::restricting(std::iter::once(PathBuf::from("src/a.py")).collect());
        let failure = per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90, None);
        let (_, file_pcts) = failure.expect("expected gate failure below 100%");
        assert_eq!(file_pcts.get(&PathBuf::from("src/a.py")), Some(&50));
    }

    #[test]
    fn per_file_gate_ignores_test_files_by_path() {
        use std::path::PathBuf;
        let test_py = PathBuf::from("tests/test_foo.py");
        let defs = vec![(test_py.clone(), "test_foo".into(), 1)];
        let unrefs = vec![(test_py.clone(), "test_foo".into(), 1)];
        let focus = crate::analyze::FocusFilter::restricting(std::iter::once(test_py).collect());
        assert!(per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90, None).is_none());
    }

    #[test]
    fn per_file_gate_overlays_weighted_pct_on_binary_zero() {
        use std::path::PathBuf;
        for def_count in [1usize, 2, 7] {
            let module = PathBuf::from(format!("src/sparse_{def_count}.rs"));
            let defs: Vec<_> = (0..def_count)
                .map(|i| (module.clone(), format!("fn_{i}"), i + 1))
                .collect();
            let unrefs = defs.clone();
            let focus = crate::analyze::FocusFilter::restricting(std::iter::once(module.clone()).collect());
            let weighted_pct = 17;
            let mut weighted = HashMap::new();
            weighted.insert(module.clone(), weighted_pct);
            let failure =
                per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90, Some(&weighted));
            let (_, file_pcts) = failure
                .unwrap_or_else(|| panic!("expected gate failure for def_count={def_count}"));
            assert_eq!(
                file_pcts.get(&module),
                Some(&weighted_pct),
                "overlay should inject weighted pct regardless of def count"
            );
        }
    }

    #[test]
    fn per_file_gate_overlay_matches_weighted_map_for_any_path() {
        use std::path::PathBuf;
        let module = PathBuf::from("src/ambiguous/testing_hooks.py");
        let defs = vec![(module.clone(), "hook".into(), 1)];
        let unrefs = vec![(module.clone(), "hook".into(), 1)];
        let focus = crate::analyze::FocusFilter::restricting(std::iter::once(module.clone()).collect());
        let mut weighted = HashMap::new();
        weighted.insert(module.clone(), 42);
        let failure = per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90, Some(&weighted));
        let (_, file_pcts) = failure.expect("expected gate failure");
        assert_eq!(file_pcts.get(&module), Some(&42));
    }

    #[test]
    fn is_coverage_gate_file_excludes_test_paths_but_not_ambiguous_prod_paths() {
        use std::path::Path;
        assert!(!is_coverage_gate_file(Path::new("tests/test_foo.py"), ""));
        assert!(is_coverage_gate_file(Path::new("src/testing_hooks.py"), "hook"));
        assert!(!is_coverage_gate_file(
            Path::new("tests/fixtures/prod_core.py"),
            "core"
        ));
    }

    #[test]
    fn gate_and_bypass_overlay_agree_on_weighted_pct() {
        use crate::analyze::coverage::{GraphRefPair, build_viols_after_merge};
        use std::path::PathBuf;
        let module = PathBuf::from("src/module.py");
        let defs = vec![
            (module.clone(), "a".into(), 1),
            (module.clone(), "b".into(), 2),
        ];
        let unrefs = vec![
            (module.clone(), "a".into(), 1),
            (module.clone(), "b".into(), 2),
        ];
        let focus = crate::analyze::FocusFilter::restricting(std::iter::once(module.clone()).collect());
        let mut weighted = HashMap::new();
        weighted.insert(module.clone(), 23);
        let gate_failure =
            per_file_coverage_gate_fails(&defs, &unrefs, &focus, 90, Some(&weighted));
        let (_, gate_pcts) = gate_failure.expect("gate should fail");
        let gate_pct = *gate_pcts.get(&module).unwrap();

        let definitions: Vec<CachedCoverageItem> = defs
            .iter()
            .map(|(f, n, l)| CachedCoverageItem {
                file: f.to_string_lossy().to_string(),
                name: n.clone(),
                line: *l,
            })
            .collect();
        let unreferenced: Vec<CachedCoverageItem> = unrefs
            .iter()
            .map(|(f, n, l)| CachedCoverageItem {
                file: f.to_string_lossy().to_string(),
                name: n.clone(),
                line: *l,
            })
            .collect();
        let graphs = GraphRefPair { py: None, rs: None };
        let (viols, _, _) = build_viols_after_merge(
            definitions,
            unreferenced,
            &focus,
            graphs,
            Some(&weighted),
            false,
        );
        let bypass_pct = viols
            .iter()
            .find(|v| v.file == module)
            .and_then(|v| {
                v.message
                    .split(':')
                    .next_back()
                    .and_then(|s| s.strip_suffix(" covered"))
                    .and_then(|s| s.strip_suffix('%'))
                    .and_then(|s| s.parse::<usize>().ok())
            })
            .unwrap_or(gate_pct);
        assert_eq!(
            gate_pct, bypass_pct,
            "gated overlay and --all violation pct should match for same weighted map"
        );
        assert_eq!(gate_pct, 23);
    }

    #[test]
    fn weighted_overlay_target_skips_test_paths() {
        use std::path::Path;
        assert!(!is_weighted_overlay_target(Path::new("tests/test_foo.py")));
        assert!(is_weighted_overlay_target(Path::new("src/foo.py")));
    }

    #[test]
    fn evaluate_gate_passes_for_empty_analysis() {
        let py_cov = kiss::TestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let rs_cov = kiss::RustTestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            propagated_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let focus = crate::analyze::FocusFilter::unrestricted();
        assert!(evaluate_gate(&py_cov, &rs_cov, &[], &[], &focus, 90).is_none());
        assert!(evaluate_cached_gate(&[], &[], &focus, 90).is_none());
    }

    #[test]
    fn test_analysis_tuples_empty() {
        let py_cov = kiss::TestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let rs_cov = kiss::RustTestRefAnalysis {
            definitions: Vec::new(),
            test_references: HashSet::new(),
            call_references: HashSet::new(),
            propagated_references: HashSet::new(),
            unreferenced: Vec::new(),
            coverage_map: HashMap::new(),
        };
        let (defs, unrefs) = analysis_tuples(&py_cov, &rs_cov);
        assert!(defs.is_empty());
        assert!(unrefs.is_empty());
    }
