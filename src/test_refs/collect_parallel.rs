use super::collect::{
    collect_definitions, collect_test_functions_with_refs,
    collect_test_functions_with_refs_for_coverage_map,
};
use super::collect_test_file::{
    collect_all_test_file_data, collect_all_test_file_data_for_coverage_map,
};
use super::detection::is_python_test_file;
use super::{CodeDefinition, PerTestUsage};
use crate::parsing::ParsedFile;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

type CollectedRefs = (
    Vec<CodeDefinition>,
    HashSet<String>,
    HashSet<String>,
    HashMap<String, HashSet<String>>,
    PerTestUsage,
);

fn empty_collected() -> CollectedRefs {
    (
        Vec::new(),
        HashSet::new(),
        HashSet::new(),
        HashMap::new(),
        PerTestUsage::new(),
    )
}

fn merge_collected(
    (mut defs, mut t_refs, mut u_refs, mut i_binds, mut pt): CollectedRefs,
    (defs2, t_refs2, u_refs2, i_binds2, pt2): CollectedRefs,
) -> CollectedRefs {
    defs.extend(defs2);
    t_refs.extend(t_refs2);
    u_refs.extend(u_refs2);
    for (module, names) in i_binds2 {
        i_binds.entry(module).or_default().extend(names);
    }
    pt.extend(pt2);
    (defs, t_refs, u_refs, i_binds, pt)
}

#[derive(Copy, Clone)]
pub(crate) enum ParallelRefsKind {
    Gate { need_coverage_map: bool },
    CoverageCalibration,
}

pub(crate) fn collect_refs_parallel(
    parsed_files: &[&ParsedFile],
    need_coverage_map: bool,
) -> CollectedRefs {
    collect_refs_parallel_with_mode(parsed_files, ParallelRefsKind::Gate {
        need_coverage_map,
    })
}

pub(crate) fn collect_refs_parallel_for_coverage_map(
    parsed_files: &[&ParsedFile],
) -> CollectedRefs {
    collect_refs_parallel_with_mode(parsed_files, ParallelRefsKind::CoverageCalibration)
}

pub(crate) fn collect_refs_parallel_with_mode(
    parsed_files: &[&ParsedFile],
    kind: ParallelRefsKind,
) -> CollectedRefs {
    let (need_coverage_map, calibration) = match kind {
        ParallelRefsKind::Gate {
            need_coverage_map,
        } => (need_coverage_map, false),
        ParallelRefsKind::CoverageCalibration => (true, true),
    };
    parsed_files
        .par_iter()
        .map(|parsed| {
            let mut r = empty_collected();
            if is_python_test_file(parsed) {
                if calibration {
                    collect_all_test_file_data_for_coverage_map(
                        parsed.tree.root_node(),
                        &parsed.source,
                        &mut r.1,
                        &mut r.2,
                        &mut r.3,
                    );
                } else {
                    collect_all_test_file_data(
                        parsed.tree.root_node(),
                        &parsed.source,
                        &mut r.1,
                        &mut r.2,
                        &mut r.3,
                    );
                }
                if need_coverage_map {
                    let mut test_funcs = Vec::new();
                    if calibration {
                        collect_test_functions_with_refs_for_coverage_map(
                            parsed.tree.root_node(),
                            &parsed.source,
                            "",
                            &mut test_funcs,
                        );
                    } else {
                        collect_test_functions_with_refs(
                            parsed.tree.root_node(),
                            &parsed.source,
                            "",
                            &mut test_funcs,
                        );
                    }
                    r.4 = vec![(parsed.path.clone(), test_funcs)];
                }
            } else {
                collect_definitions(
                    parsed.tree.root_node(),
                    &parsed.source,
                    &parsed.path,
                    &mut r.0,
                    false,
                    None,
                );
            }
            r
        })
        .fold(empty_collected, merge_collected)
        .reduce(empty_collected, merge_collected)
}

#[must_use]
pub fn test_functions_in(parsed: &ParsedFile) -> Vec<String> {
    let mut out = Vec::new();
    collect_test_functions_with_refs(
        parsed.tree.root_node(),
        &parsed.source,
        "",
        &mut out,
    );
    out.into_iter().map(|(id, _)| id).collect()
}

#[cfg(test)]
mod collect_parallel_tests {
    use super::*;
    use crate::parsing::{create_parser, parse_file};
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_functions_in_lists_test_defs() {
        let mut tmp = NamedTempFile::with_suffix(".py").unwrap();
        writeln!(tmp, "def test_sample(): pass").unwrap();
        let mut parser = create_parser().expect("parser");
        let parsed = parse_file(&mut parser, tmp.path()).expect("parse");
        let names = test_functions_in(&parsed);
        assert!(names.iter().any(|n| n == "test_sample"));
    }

    #[test]
    fn collect_refs_parallel_merges_source_and_test_files() {
        let mut src = NamedTempFile::with_suffix(".py").unwrap();
        writeln!(src, "def helper(): pass").unwrap();
        let mut test = tempfile::Builder::new()
            .prefix("test_")
            .suffix(".py")
            .tempfile()
            .unwrap();
        writeln!(
            test,
            "from helper import helper\ndef test_helper():\n    helper()"
        )
        .unwrap();
        let mut parser = create_parser().expect("parser");
        let parsed_src = parse_file(&mut parser, src.path()).expect("parse src");
        let parsed_test = parse_file(&mut parser, test.path()).expect("parse test");
        let refs = [&parsed_src, &parsed_test];
        let (defs, test_refs, _, _, per_test) = collect_refs_parallel(&refs, true);
        assert!(defs.iter().any(|d| d.name == "helper"));
        assert!(test_refs.contains("helper"));
        assert_eq!(per_test.len(), 1);
        assert!(
            per_test[0]
                .1
                .iter()
                .any(|(name, usage)| name == "test_helper" && usage.contains("helper"))
        );
    }

    #[test]
    fn collect_refs_parallel_for_coverage_map_calibration_branch() {
        let mut src = NamedTempFile::with_suffix(".py").unwrap();
        writeln!(src, "def api():\n    pass").unwrap();
        let mut test = NamedTempFile::with_suffix("_test.py").unwrap();
        writeln!(test, "def test_api():\n    api()").unwrap();
        let mut parser = create_parser().expect("parser");
        let parsed_src = parse_file(&mut parser, src.path()).expect("parse");
        let parsed_test = parse_file(&mut parser, test.path()).expect("parse");
        let refs = [&parsed_src, &parsed_test];
        let (_, _, usage, _, per_test) = collect_refs_parallel_for_coverage_map(&refs);
        assert!(usage.contains("api"));
        assert_eq!(per_test.len(), 1);
    }

    #[test]
    fn collect_refs_parallel_with_mode_empty_and_flags() {
        let empty = collect_refs_parallel_with_mode(&[], ParallelRefsKind::CoverageCalibration);
        assert!(empty.0.is_empty());
        let empty2 = collect_refs_parallel_with_mode(
            &[],
            ParallelRefsKind::Gate {
                need_coverage_map: false,
            },
        );
        assert!(empty2.0.is_empty());
    }
}
