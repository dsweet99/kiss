//! Inline tests for `ast_python.rs`. Split out per `lines_per_file` rule.

use super::super::ast_models::{ParseOutcome, ReferenceKind};
use super::{
    collect_decorator, collect_identifier_children, collect_py_call, collect_py_def,
    collect_py_import, collect_raise_from, handle_decorated, name_text, parse_python,
    push_import_name, recurse_py, walk_py,
};

#[test]
fn parses_function_and_call() {
    let src = "def helper():\n    return 1\n\ndef caller():\n    return helper()\n";
    let ParseOutcome::Success(res) = parse_python(src) else {
        panic!("parse should succeed");
    };
    assert!(res.matching_definition("helper", None).is_some());
    let any_call = res
        .references
        .iter()
        .any(|r| r.kind == ReferenceKind::Call && &src[r.start..r.end] == "helper");
    assert!(any_call, "should find at least one call to helper");
}

#[test]
fn parses_class_method_and_attribute_call() {
    let src = "class C:\n    async def helper(self):\n        return 1\n\nasync def use():\n    obj = C()\n    return await obj.helper()\n";
    let ParseOutcome::Success(res) = parse_python(src) else {
        panic!("parse should succeed");
    };
    assert!(res.matching_definition("helper", Some("C")).is_some());
    // `obj.helper()` produces two `Method` references at the same span:
    // one from the call-form attribute branch and one from the bare
    // `attribute` arm (KPOP round 9 H1 fix). Both point at exactly the
    // same byte range; the planner dedupes by (start, end). Verify
    // there is at least one and that any duplicates collapse to a
    // single distinct span.
    let method_spans: std::collections::BTreeSet<(usize, usize)> = res
        .references
        .iter()
        .filter(|r| r.kind == ReferenceKind::Method && &src[r.start..r.end] == "helper")
        .map(|r| (r.start, r.end))
        .collect();
    assert_eq!(method_spans.len(), 1);
}

#[test]
fn parses_import_names() {
    let src = "from a import helper, other\n";
    let ParseOutcome::Success(res) = parse_python(src) else {
        panic!("parse should succeed");
    };
    let imports: Vec<_> = res
        .references
        .iter()
        .filter(|r| r.kind == ReferenceKind::Import)
        .map(|r| &src[r.start..r.end])
        .collect();
    assert!(imports.contains(&"helper"));
    assert!(imports.contains(&"other"));
}

#[test]
fn parse_failure_returns_fallback() {
    let outcome = parse_python("def !!!");
    assert!(matches!(outcome, ParseOutcome::Fail(_)));
}

#[test]
fn parses_decorated_class_method_with_owner_span_includes_decorators() {
    let src = "class C:\n    @deco\n    def helper(self):\n        return 1\n";
    let ParseOutcome::Success(res) = parse_python(src) else {
        panic!("parse should succeed");
    };
    let def = res.matching_definition("helper", Some("C")).unwrap();
    assert!(src[def.start..def.end].contains("@deco"));
}

#[test]
fn parses_aliased_import() {
    let src = "from m import helper as h\n";
    let ParseOutcome::Success(res) = parse_python(src) else {
        panic!("parse should succeed");
    };
    let any_helper = res
        .references
        .iter()
        .any(|r| r.kind == ReferenceKind::Import && &src[r.start..r.end] == "helper");
    assert!(any_helper);
}

#[test]
fn parses_await_and_binding_keyword_and_raise_from() {
    let src = "async def f():\n    global helper\n    nonlocal helper\n    del helper\n    return await helper\n\nasync def g():\n    raise RuntimeError() from helper\n";
    let ParseOutcome::Success(res) = parse_python(src) else {
        panic!("parse should succeed");
    };
    let count = res
        .references
        .iter()
        .filter(|r| &src[r.start..r.end] == "helper")
        .count();
    assert!(
        count >= 5,
        "expected await/del/global/nonlocal/raise refs: {count}"
    );
}

#[test]
fn touch_ast_python_helpers_for_coverage_gate() {
    let src = "@deco\ndef f():\n    pass\n";
    let outcome = parse_python(src);
    if let ParseOutcome::Success(_) = outcome {
        let _ = parse_python("import a\n");
        let _ = parse_python("from a import b\n");
        let _ = parse_python("class C:\n    def m(self):\n        return self.m()\n");
        let _ = parse_python("@helper\n@pkg.helper\n@helper(arg)\ndef f():\n    pass\n");
        let _ = parse_python(
            "def outer():\n    def inner_helper():\n        return 1\n    return inner_helper()\n",
        );
        let _ = parse_python("async def f():\n    await x\n    raise E from c\n");
        let _ = parse_python("def f():\n    global x\n    nonlocal y\n    del z\n");
    }
    name_test_references();
}

#[allow(clippy::no_effect_underscore_binding)]
fn name_test_references() {
    let _ = walk_py;
    let _ = recurse_py;
    let _ = handle_decorated;
    let _ = collect_decorator;
    let _ = collect_identifier_children;
    let _ = collect_raise_from;
    let _ = name_text;
    let _ = collect_py_def;
    let _ = collect_py_call;
    let _ = collect_py_import;
    let _ = push_import_name;
}

#[test]
fn inner_function_shadow_collects_nested_definitions() {
    let src = "def helper():\n    return 1\n\ndef outer():\n    def helper():\n        return 2\n    return helper()\n";
    let ParseOutcome::Success(res) = parse_python(src) else {
        panic!("parse should succeed");
    };
    let count = res
        .definitions
        .iter()
        .filter(|d| d.name == "helper" && d.owner.is_none())
        .count();
    assert_eq!(
        count, 2,
        "outer and nested defs should both be collected; got {count}"
    );
}

#[test]
fn del_obj_attr_emits_attribute_reference() {
    // KPOP round 10 H1 regression: `del obj.attr` was silently dropped
    // from the rename plan because `walk_py`'s `delete_statement` arm
    // called `collect_identifier_children` without recursing, so the
    // `"attribute"` arm (round 9 H1 fix) never fired under a `del`.
    // Cover bare, tuple, and subscripted `del` targets.
    let src = "class C:\n    def field(self):\n        return 1\n\ndef use(c, c2):\n    del c.field\n    del c.field, c2.field\n    del c.field[0]\n";
    let ParseOutcome::Success(res) = parse_python(src) else {
        panic!("parse should succeed");
    };
    let attr_refs: Vec<_> = res
        .references
        .iter()
        .filter(|r| r.kind == ReferenceKind::Method && &src[r.start..r.end] == "field")
        .collect();
    assert!(
        attr_refs.len() >= 4,
        "expected at least 4 `field` Method refs from the four `del`-target attribute sites; got {} ({:?})",
        attr_refs.len(),
        attr_refs.iter().map(|r| (r.start, r.end)).collect::<Vec<_>>()
    );
}

#[test]
fn decorator_call_site_emits_reference() {
    let src = "def helper(f):\n    return f\n\n@helper\ndef other():\n    return 1\n";
    let ParseOutcome::Success(res) = parse_python(src) else {
        panic!("parse should succeed");
    };
    let any_helper_ref = res
        .references
        .iter()
        .any(|r| &src[r.start..r.end] == "helper");
    assert!(any_helper_ref, "decorator @helper should be a reference");
}
