use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::edit::{MvPlan, PlannedEdit};
use super::opts::MvRequest;

use crate::symbol_mv_support::{
    MoveEditsParams, ReferenceRenameParams, SourceRenameParams, build_move_edits,
    collect_reference_edits, collect_source_rename_edits,
};

const fn empty_plan() -> MvPlan {
    MvPlan {
        files: Vec::new(),
        edits: Vec::new(),
    }
}

struct AppendReferenceCtx<'a> {
    req: &'a MvRequest,
    source_canonical: &'a Path,
    old_name: &'a str,
    files: &'a mut BTreeSet<PathBuf>,
    edits: &'a mut Vec<PlannedEdit>,
}

fn append_reference_edits(ctx: &mut AppendReferenceCtx<'_>) {
    let owner = ctx
        .req
        .query
        .member
        .as_ref()
        .map(|_| ctx.req.query.symbol.as_str());
    for path in crate::symbol_mv_support::gather_candidate_files(
        &ctx.req.paths,
        &ctx.req.ignore,
        ctx.req.query.language,
    ) {
        let canonical = crate::rust_include::canonical_path(&path);
        if canonical == ctx.source_canonical {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if owner.is_none()
            && ctx.req.query.language == crate::Language::Python
            && has_python_top_level_definition(&content, ctx.old_name)
        {
            continue;
        }
        let ref_edits = collect_reference_edits(&ReferenceRenameParams {
            path: &path,
            content: &content,
            old_name: ctx.old_name,
            new_name: &ctx.req.new_name,
            owner,
            language: ctx.req.query.language,
        });
        if !ref_edits.is_empty() {
            ctx.files.insert(path);
            ctx.edits.extend(ref_edits);
        }
    }
}

const fn is_python_identifier_boundary(next: Option<char>) -> bool {
    matches!(next, None | Some(' ' | '\t' | '(' | ':' | '\r' | '\n'))
}

fn has_python_top_level_definition(content: &str, old_name: &str) -> bool {
    for line in content.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let body = trimmed
            .strip_prefix("async ")
            .map_or(trimmed, |after_async| after_async);
        if let Some(rest) = body.strip_prefix("def ") {
            if rest.starts_with(old_name) {
                let next = rest.get(old_name.len()..).and_then(|s| s.chars().next());
                if is_python_identifier_boundary(next) {
                    return true;
                }
            }
        } else if let Some(rest) = body.strip_prefix("class ")
            && rest.starts_with(old_name)
        {
            let next = rest.get(old_name.len()..).and_then(|s| s.chars().next());
            if is_python_identifier_boundary(next) {
                return true;
            }
        }
    }
    false
}

struct AppendMoveCtx<'a> {
    req: &'a MvRequest,
    source_path: &'a Path,
    source_content: &'a str,
    old_name: &'a str,
    files: &'a mut BTreeSet<PathBuf>,
    edits: &'a mut Vec<PlannedEdit>,
    def_span: Option<crate::symbol_mv_support::DefinitionSpan>,
}

fn append_move_edits_if_any(ctx: &mut AppendMoveCtx<'_>) {
    let Some((dest_path, remove_edit, insert_edit)) = build_move_edits(&MoveEditsParams {
        source_path: ctx.source_path,
        source_content: ctx.source_content,
        old_name: ctx.old_name,
        new_name: &ctx.req.new_name,
        def_span: ctx.def_span,
        dest: ctx.req.to.as_ref(),
    }) else {
        return;
    };
    ctx.files.insert(dest_path);
    ctx.edits.push(remove_edit);
    ctx.edits.push(insert_edit);
}

fn finalize_plan(files: BTreeSet<PathBuf>, mut edits: Vec<PlannedEdit>) -> MvPlan {
    edits.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.start_byte.cmp(&b.start_byte))
    });
    MvPlan {
        files: files.into_iter().collect(),
        edits,
    }
}

pub fn plan_edits(req: &MvRequest) -> MvPlan {
    let _guard = crate::symbol_mv_support::PlanInvocationGuard::enter();
    let old_name = req.query.old_name();
    let source_path = &req.query.path;
    let source_canonical = crate::rust_include::canonical_path(source_path);
    let Ok(source_content) = fs::read_to_string(source_path) else {
        return empty_plan();
    };

    let mut files = BTreeSet::new();
    let owner = req.query.member.as_ref().map(|_| req.query.symbol.as_str());
    let def_span = crate::symbol_mv_support::find_definition_span(
        &source_content,
        old_name,
        owner,
        req.query.language,
        source_path,
    );

    let mut edits = collect_source_rename_edits(&SourceRenameParams {
        source_path,
        source_content: &source_content,
        old_name,
        new_name: &req.new_name,
        owner,
        language: req.query.language,
        def_span,
        moving: req.to.is_some(),
    });
    files.insert(source_path.clone());

    append_reference_edits(&mut AppendReferenceCtx {
        req,
        source_canonical: &source_canonical,
        old_name,
        files: &mut files,
        edits: &mut edits,
    });
    append_move_edits_if_any(&mut AppendMoveCtx {
        req,
        source_path,
        source_content: &source_content,
        old_name,
        files: &mut files,
        edits: &mut edits,
        def_span,
    });

    finalize_plan(files, edits)
}

#[cfg(test)]
mod plan_coverage {
    use super::*;
    use crate::symbol_mv::edit::EditKind;
    use crate::symbol_mv::{MvRequest, parse_mv_query};
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    #[test]
    fn empty_plan_has_no_files_or_edits() {
        let plan = empty_plan();
        assert!(plan.files.is_empty());
        assert!(plan.edits.is_empty());
    }

    #[test]
    fn has_python_top_level_definition_detects_def_and_class() {
        assert!(has_python_top_level_definition("def foo(): pass\n", "foo"));
        assert!(has_python_top_level_definition("class Foo: pass\n", "Foo"));
        assert!(!has_python_top_level_definition(
            "def food(): pass\n",
            "foo"
        ));
        assert!(!has_python_top_level_definition(
            "  def foo(): pass\n",
            "foo"
        ));
    }

    #[test]
    fn is_python_identifier_boundary_rejects_continued_identifiers() {
        assert!(is_python_identifier_boundary(None));
        assert!(is_python_identifier_boundary(Some('(')));
        assert!(!is_python_identifier_boundary(Some('a')));
    }

    #[test]
    fn finalize_plan_sorts_by_path_then_start_byte() {
        let mut files = BTreeSet::new();
        files.insert(PathBuf::from("b.py"));
        files.insert(PathBuf::from("a.py"));
        let edits = vec![
            PlannedEdit {
                path: PathBuf::from("b.py"),
                start_byte: 10,
                end_byte: 11,
                line: 1,
                old_snippet: "x".into(),
                new_snippet: "y".into(),
                kind: EditKind::Reference,
            },
            PlannedEdit {
                path: PathBuf::from("a.py"),
                start_byte: 0,
                end_byte: 1,
                line: 1,
                old_snippet: "x".into(),
                new_snippet: "y".into(),
                kind: EditKind::Reference,
            },
            PlannedEdit {
                path: PathBuf::from("b.py"),
                start_byte: 0,
                end_byte: 1,
                line: 1,
                old_snippet: "x".into(),
                new_snippet: "y".into(),
                kind: EditKind::Reference,
            },
        ];
        let plan = finalize_plan(files, edits);
        assert_eq!(plan.edits[0].path, PathBuf::from("a.py"));
        assert_eq!(plan.edits[1].start_byte, 0);
        assert_eq!(plan.edits[2].start_byte, 10);
    }

    #[test]
    fn plan_edits_returns_empty_plan_when_source_missing() {
        let query = parse_mv_query("missing_file_for_plan_test.py::foo").unwrap();
        let req = MvRequest {
            query,
            new_name: "bar".into(),
            paths: vec![],
            to: None,
            ignore: vec![],
        };
        let plan = plan_edits(&req);
        assert!(plan.files.is_empty());
        assert!(plan.edits.is_empty());
    }

    #[test]
    fn plan_edits_collects_reference_edits_for_caller_file() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("a.py");
        let caller = tmp.path().join("caller.py");
        std::fs::write(&source, "def foo():\n    return 1\n").unwrap();
        std::fs::write(&caller, "from a import foo\nfoo()\n").unwrap();
        let query = parse_mv_query(&format!("{}::foo", source.display())).unwrap();
        let req = MvRequest {
            query,
            new_name: "bar".into(),
            paths: vec![tmp.path().display().to_string()],
            to: None,
            ignore: vec![],
        };
        let plan = plan_edits(&req);
        assert!(plan.edits.iter().any(|e| e.old_snippet == "foo"));
        assert!(plan.files.iter().any(|p| p == &source));
    }

    #[test]
    fn append_reference_edits_skips_canonical_source_file() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("a.py");
        std::fs::write(&source, "def foo(): pass\n").unwrap();
        let query = parse_mv_query(&format!("{}::foo", source.display())).unwrap();
        let req = MvRequest {
            query,
            new_name: "bar".into(),
            paths: vec![tmp.path().display().to_string()],
            to: None,
            ignore: vec![],
        };
        let source_canonical = crate::rust_include::canonical_path(&source);
        let mut files = BTreeSet::new();
        let mut edits = Vec::new();
        append_reference_edits(&mut AppendReferenceCtx {
            req: &req,
            source_canonical: &source_canonical,
            old_name: "foo",
            files: &mut files,
            edits: &mut edits,
        });
        assert!(!files.contains(&source));
    }

    #[test]
    fn append_move_edits_if_any_noops_without_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("a.py");
        std::fs::write(&source, "def foo(): return 1\n").unwrap();
        let query = parse_mv_query(&format!("{}::foo", source.display())).unwrap();
        let req = MvRequest {
            query,
            new_name: "bar".into(),
            paths: vec![],
            to: None,
            ignore: vec![],
        };
        let source_content = std::fs::read_to_string(&source).unwrap();
        let def_span = crate::symbol_mv_support::find_definition_span(
            &source_content,
            "foo",
            None,
            crate::Language::Python,
            &source,
        );
        let mut files = BTreeSet::new();
        let mut edits = Vec::new();
        append_move_edits_if_any(&mut AppendMoveCtx {
            req: &req,
            source_path: &source,
            source_content: &source_content,
            old_name: "foo",
            files: &mut files,
            edits: &mut edits,
            def_span,
        });
        assert!(edits.is_empty());
    }
}

#[cfg(test)]
mod coverage_witness {
    use super::*;
    use crate::symbol_mv::{MvRequest, parse_mv_query};
    use std::collections::BTreeSet;

    impl AppendReferenceCtx<'_> {
        fn witness() {}
    }

    impl AppendMoveCtx<'_> {
        fn witness() {}
    }

    #[test]
    fn witness_append_context_types() {
        AppendReferenceCtx::witness();
        AppendMoveCtx::witness();
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("a.py");
        std::fs::write(&source, "def foo(): pass\n").unwrap();
        let query = parse_mv_query(&format!("{}::foo", source.display())).unwrap();
        let req = MvRequest {
            query,
            new_name: "bar".into(),
            paths: vec![tmp.path().display().to_string()],
            to: None,
            ignore: vec![],
        };
        let source_canonical = crate::rust_include::canonical_path(&source);
        let mut files = BTreeSet::new();
        let mut edits = Vec::new();
        append_reference_edits(&mut AppendReferenceCtx {
            req: &req,
            source_canonical: &source_canonical,
            old_name: "foo",
            files: &mut files,
            edits: &mut edits,
        });
        append_move_edits_if_any(&mut AppendMoveCtx {
            req: &req,
            source_path: &source,
            source_content: "def foo(): pass\n",
            old_name: "foo",
            files: &mut files,
            edits: &mut edits,
            def_span: None,
        });
        assert!(edits.is_empty());
    }
}
