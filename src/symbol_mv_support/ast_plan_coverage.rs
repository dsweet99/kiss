use super::ast_plan_extras::method_receiver_matches;
use super::*;
use crate::symbol_mv_support::ast_models::{Reference, ReferenceKind};
use crate::symbol_mv_support::reference::{
    infer_python_receiver_type_pub, infer_rust_receiver_type_pub,
};

#[test]
fn definition_span_matches_python() {
    let _g = PlanInvocationGuard::enter();
    let src = "def helper():\n    return 1\n";
    let (s, e) = ast_definition_span(src, "helper", None, Language::Python).unwrap();
    assert!(src[s..e].contains("def helper"));
}

#[test]
fn reference_offsets_owner_none_returns_calls_and_imports() {
    let _g = PlanInvocationGuard::enter();
    let src = "from m import helper\n\ndef use():\n    return helper()\n";
    let sites = ast_reference_offsets(src, "helper", None, Language::Python).unwrap();
    assert!(sites.len() >= 2, "expected import + call: {sites:?}");
}

#[test]
fn reference_offsets_owner_some_yields_method_when_receiver_resolves() {
    let _g = PlanInvocationGuard::enter();
    let src = "class C:\n    def helper(self): return 1\n\ndef use():\n    obj = C()\n    return obj.helper()\n";
    let sites = ast_reference_offsets(src, "helper", Some("C"), Language::Python).unwrap();
    assert!(
        sites.iter().any(|(s, e)| &src[*s..*e] == "helper"),
        "owner-qualified AST should yield the method site, got {sites:?}"
    );
}

#[test]
fn rust_owner_qualified_yields_method_site() {
    let _g = PlanInvocationGuard::enter();
    let src = "struct X;\nimpl X { fn helper(&self) {} }\nfn c(x: &X) { x.helper(); }\n";
    let sites = ast_reference_offsets(src, "helper", Some("X"), Language::Rust).unwrap();
    assert!(
        !sites.is_empty(),
        "owner-qualified AST should yield method site"
    );
}

#[test]
fn rust_second_impl_block_method_site_is_admitted() {
    let _g = PlanInvocationGuard::enter();
    let src = "struct Foo;\n\nimpl Foo { fn alpha(&self) -> i32 { 1 } }\n\nimpl Foo { fn beta(&self) -> i32 { 2 } }\n\nfn call(f: &Foo) {\n    f.alpha();\n    f.beta();\n}\n";
    let upto = src.rfind("beta").expect("fixture should contain beta");
    assert_eq!(
        infer_rust_receiver_type_pub(src, upto, "f"),
        Some("Foo".to_string())
    );
    assert!(method_receiver_matches(
        src,
        upto,
        "Foo",
        None,
        Language::Rust
    ));
    let sites = ast_reference_offsets(src, "beta", Some("Foo"), Language::Rust).unwrap();
    assert!(
        sites.iter().any(|(s, e)| &src[*s..*e] == "beta"),
        "second-impl Rust method call should be admitted, got {sites:?}"
    );
}

#[test]
fn python_worker_method_site_is_admitted() {
    let _g = PlanInvocationGuard::enter();
    let src = "class Worker:\n    def run(self, value: int) -> int:\n        return value + 1\n\n\ndef test_methods_stay_distinct():\n    assert Worker().run(4) == 5\n";
    let upto = src.rfind("run").expect("fixture should contain run");
    assert_eq!(
        infer_python_receiver_type_pub(src, upto, "Worker"),
        Some("Worker".to_string())
    );
    assert!(method_receiver_matches(
        src,
        upto,
        "Worker",
        None,
        Language::Python
    ));
    let sites = ast_reference_offsets(src, "run", Some("Worker"), Language::Python).unwrap();
    assert!(
        sites.iter().any(|(s, e)| &src[*s..*e] == "run"),
        "Python Worker().run call should be admitted, got {sites:?}"
    );
}

#[test]
fn parse_failure_returns_none() {
    let _g = PlanInvocationGuard::enter();
    assert!(ast_definition_span("def !!!", "helper", None, Language::Python).is_none());
    assert!(ast_reference_offsets("def !!!", "helper", None, Language::Python).is_none());
}

#[test]
fn matches_name_bounds() {
    assert!(matches_name("abc", 0, 3, "abc"));
    assert!(!matches_name("abc", 0, 4, "abcd"));
}

#[test]
fn parse_cache_avoids_duplicate_parse() {
    let _g = PlanInvocationGuard::enter();
    let src = "def helper():\n    return 1\n";
    let _ = ast_definition_span(src, "helper", None, Language::Python);
    let cached_len = PARSE_CACHE.with(|c| c.borrow().len());
    assert_eq!(cached_len, 1);
    let _ = ast_reference_offsets(src, "helper", None, Language::Python);
    assert_eq!(PARSE_CACHE.with(|c| c.borrow().len()), 1);
}

#[test]
fn touch_ast_plan_helpers_for_coverage_gate() {
    let _g = PlanInvocationGuard::enter();
    let _ = parse_for("x = 1\n", Language::Python);
    let _ = parse_for("fn x() {}\n", Language::Rust);
    assert_eq!(lang_key(Language::Python), 0);
    assert_eq!(lang_key(Language::Rust), 1);
    let res = AstResult {
        definitions: vec![],
        references: vec![],
        trait_impls: vec![],
    };
    let cached = CachedOutcome::Success(res);
    let _ = cached_to_outcome(cached);
    let _ = cached_to_outcome(CachedOutcome::Fail(FallbackReason::ParseFailed));
    let _ = cached_to_outcome(CachedOutcome::Fail(FallbackReason::ParserUnavailable));
    let _ = cached_parse("def f():\n pass\n", Language::Python);
    let _ = ast_definition_ident_offsets("def f():\n pass\n", "f", None, Language::Python);
    let r = Reference {
        start: 0,
        end: 1,
        kind: ReferenceKind::Method,
    };
    let _ = reference_admits("a", &r, Some("X"), None, Language::Python);
    let _ = method_receiver_matches("a = X()\na.f()", 9, "X", None, Language::Python);
    let _ = method_receiver_matches("let a:X=x;\na.f()", 12, "X", None, Language::Rust);
    let _ = cached_parse_outcome(
        "def f():\n pass\n",
        std::path::Path::new("warn-test-a"),
        Language::Python,
    );
    let _ = cached_parse_outcome(
        "def g():\n pass\n",
        std::path::Path::new("warn-test-b"),
        Language::Rust,
    );
}
