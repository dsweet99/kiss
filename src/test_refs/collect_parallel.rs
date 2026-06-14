use super::collect::{
    collect_all_test_file_data, collect_definitions, collect_test_functions_with_refs,
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
    HashSet<String>,
    HashMap<String, HashSet<String>>,
    HashMap<String, String>,
    PerTestUsage,
);

fn empty_collected() -> CollectedRefs {
    (
        Vec::new(),
        HashSet::new(),
        HashSet::new(),
        HashSet::new(),
        HashMap::new(),
        HashMap::new(),
        PerTestUsage::new(),
    )
}

fn merge_collected(
    (mut defs, mut t_refs, mut u_refs, mut c_refs, mut i_binds, mut a_binds, mut pt): CollectedRefs,
    (defs2, t_refs2, u_refs2, c_refs2, i_binds2, a_binds2, pt2): CollectedRefs,
) -> CollectedRefs {
    defs.extend(defs2);
    t_refs.extend(t_refs2);
    u_refs.extend(u_refs2);
    c_refs.extend(c_refs2);
    for (module, names) in i_binds2 {
        i_binds.entry(module).or_default().extend(names);
    }
    a_binds.extend(a_binds2);
    pt.extend(pt2);
    (defs, t_refs, u_refs, c_refs, i_binds, a_binds, pt)
}

pub(crate) fn collect_refs_parallel(
    parsed_files: &[&ParsedFile],
    need_coverage_map: bool,
) -> CollectedRefs {
    parsed_files
        .par_iter()
        .map(|parsed| {
            let mut r = empty_collected();
            if is_python_test_file(parsed) {
                collect_all_test_file_data(
                    parsed.tree.root_node(),
                    &parsed.source,
                    &mut r.1,
                    &mut r.2,
                    &mut r.3,
                    &mut r.4,
                    &mut r.5,
                );
                if need_coverage_map {
                    let mut test_funcs = Vec::new();
                    collect_test_functions_with_refs(
                        parsed.tree.root_node(),
                        &parsed.source,
                        "",
                        &mut test_funcs,
                    );
                    r.6 = vec![(parsed.path.clone(), test_funcs)];
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
    collect_test_functions_with_refs(parsed.tree.root_node(), &parsed.source, "", &mut out);
    out.into_iter().map(|(id, _, _)| id).collect()
}

#[cfg(test)]
mod collect_parallel_tests {
    use super::*;
    use crate::parsing::{create_parser, parse_file};
    use crate::test_refs::CodeDefinition;
    use crate::units::CodeUnitKind;
    use std::collections::{HashMap, HashSet};
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    #[test]
    fn merge_collected_combines_every_bucket() {
        let left_def = CodeDefinition {
            name: "left".into(),
            kind: CodeUnitKind::Function,
            file: PathBuf::from("left.py"),
            line: 1,
            containing_class: None,
        };
        let right_def = CodeDefinition {
            name: "right".into(),
            kind: CodeUnitKind::Function,
            file: PathBuf::from("right.py"),
            line: 1,
            containing_class: None,
        };
        let mut left_imports = HashMap::new();
        left_imports.insert("mod".into(), HashSet::from(["left".into()]));
        let mut right_imports = HashMap::new();
        right_imports.insert("mod".into(), HashSet::from(["right".into()]));

        let merged = merge_collected(
            (
                vec![left_def],
                HashSet::from(["left_ref".into()]),
                HashSet::from(["left_usage".into()]),
                HashSet::from(["left_call".into()]),
                left_imports,
                HashMap::from([("alias_l".into(), "left".into())]),
                vec![(PathBuf::from("test_l.py"), Vec::new())],
            ),
            (
                vec![right_def],
                HashSet::from(["right_ref".into()]),
                HashSet::from(["right_usage".into()]),
                HashSet::from(["right_call".into()]),
                right_imports,
                HashMap::from([("alias_r".into(), "right".into())]),
                vec![(PathBuf::from("test_r.py"), Vec::new())],
            ),
        );

        assert_eq!(merged.0.len(), 2);
        assert!(merged.1.contains("left_ref") && merged.1.contains("right_ref"));
        assert!(merged.2.contains("left_usage") && merged.2.contains("right_usage"));
        assert!(merged.3.contains("left_call") && merged.3.contains("right_call"));
        assert_eq!(merged.4["mod"].len(), 2);
        assert_eq!(merged.5["alias_r"], "right");
        assert_eq!(merged.6.len(), 2);
    }

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
        let (defs, test_refs, _, call_refs, _, _, per_test) = collect_refs_parallel(&refs, true);
        assert!(defs.iter().any(|d| d.name == "helper"));
        assert!(test_refs.contains("helper"));
        assert!(call_refs.contains("helper"));
        assert_eq!(per_test.len(), 1);
        assert!(per_test[0].1.iter().any(|(name, usage, call)| {
            name == "test_helper" && usage.contains("helper") && call.contains("helper")
        }));
    }
}
