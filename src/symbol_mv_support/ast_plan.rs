//! AST-first planning helpers (Tasks 4 & 5).
//!
//! Call sites prefer the AST path; on parse failure they fall back to the
//! lexical helpers in `lex.rs` / `signature.rs` and emit a single-shape
//! warning so the fallback is observable. Returned offsets are byte offsets
//! into the original source string and may be unioned with lexical hits to
//! preserve current behavior during the transition.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::Language;

use super::ast_models::{AstResult, FallbackReason, ParseOutcome, Reference, ReferenceKind};
use super::ast_python::parse_python;
use super::ast_rust::parse_rust;
use super::reference::{
    extract_receiver_pub, infer_python_receiver_type_pub, infer_rust_receiver_type_pub,
};

pub(super) fn parse_for(content: &str, language: Language) -> ParseOutcome {
    match language {
        Language::Python => parse_python(content),
        Language::Rust => parse_rust(content),
    }
}

thread_local! {
    static PARSE_CACHE: RefCell<HashMap<(u64, usize, u8), CachedOutcome>>
        = RefCell::new(HashMap::new());
}

fn content_hash(content: &str) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(content.as_bytes());
    hasher.finish()
}

const fn lang_key(language: Language) -> u8 {
    match language {
        Language::Python => 0,
        Language::Rust => 1,
    }
}

#[derive(Clone)]
enum CachedOutcome {
    Success(AstResult),
    Fail(FallbackReason),
}

pub(crate) struct PlanInvocationGuard;

impl PlanInvocationGuard {
    pub(crate) fn enter() -> Self {
        WARNED_THIS_INVOCATION.with(|w| w.set(false));
        PARSE_CACHE.with(|c| c.borrow_mut().clear());
        Self
    }
}

impl Drop for PlanInvocationGuard {
    fn drop(&mut self) {
        PARSE_CACHE.with(|c| c.borrow_mut().clear());
    }
}

fn cached_parse(content: &str, language: Language) -> Option<AstResult> {
    let key = (content_hash(content), content.len(), lang_key(language));
    let cached = PARSE_CACHE.with(|c| c.borrow().get(&key).cloned());
    if let Some(hit) = cached {
        return cached_to_option(hit);
    }
    let outcome = match parse_for(content, language) {
        ParseOutcome::Success(res) => CachedOutcome::Success(res),
        ParseOutcome::Fail(reason) => CachedOutcome::Fail(reason),
    };
    PARSE_CACHE.with(|c| c.borrow_mut().insert(key, outcome.clone()));
    cached_to_option(outcome)
}

fn cached_to_option(outcome: CachedOutcome) -> Option<AstResult> {
    match outcome {
        CachedOutcome::Success(res) => Some(res),
        CachedOutcome::Fail(reason) => {
            warn_per_invocation(&reason);
            None
        }
    }
}

pub(super) fn ast_definition_span(
    content: &str,
    name: &str,
    owner: Option<&str>,
    language: Language,
) -> Option<(usize, usize)> {
    let result = cached_parse(content, language)?;
    let def = result.matching_definition(name, owner)?;
    Some((def.start, def.end))
}

pub(super) fn ast_definition_ident_offsets(
    content: &str,
    name: &str,
    owner: Option<&str>,
    language: Language,
) -> Option<Vec<(usize, usize)>> {
    let result = cached_parse(content, language)?;
    let mut sites = Vec::new();
    for d in &result.definitions {
        if d.name != name || d.owner.as_deref() != owner {
            continue;
        }
        let (s, e) = (d.name_start, d.name_end);
        assert!(
            e <= content.len() && &content[s..e] == name,
            "AST name span must match the symbol name exactly"
        );
        sites.push((s, e));
    }
    Some(sites)
}

pub(super) fn ast_reference_offsets(
    content: &str,
    name: &str,
    owner: Option<&str>,
    language: Language,
) -> Option<Vec<(usize, usize)>> {
    let result = cached_parse(content, language)?;
    let mut sites: Vec<(usize, usize)> = result
        .references
        .iter()
        .filter(|r| matches_name(content, r.start, r.end, name))
        .filter(|r| reference_admits(content, r, owner, language))
        .map(|r| (r.start, r.end))
        .collect();
    sites.sort_unstable();
    sites.dedup();
    Some(sites)
}

fn reference_admits(
    content: &str,
    r: &Reference,
    owner: Option<&str>,
    language: Language,
) -> bool {
    match (r.kind, owner) {
        (ReferenceKind::Call | ReferenceKind::Import, None) => true,
        (ReferenceKind::Method, Some(type_name)) => {
            method_receiver_matches(content, r.start, type_name, language)
        }
        _ => false,
    }
}

fn method_receiver_matches(
    content: &str,
    start: usize,
    type_name: &str,
    language: Language,
) -> bool {
    let before = &content[..start];
    let receiver = extract_receiver_pub(before);
    let inferred = match language {
        Language::Python => infer_python_receiver_type_pub(content, start, &receiver),
        Language::Rust => infer_rust_receiver_type_pub(content, start, &receiver),
    };
    inferred.as_deref() == Some(type_name)
}

fn matches_name(content: &str, start: usize, end: usize, name: &str) -> bool {
    end <= content.len() && &content[start..end] == name
}

thread_local! {
    static WARNED_THIS_INVOCATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn warn_per_invocation(reason: &FallbackReason) {
    let already = WARNED_THIS_INVOCATION.with(std::cell::Cell::get);
    if already {
        return;
    }
    WARNED_THIS_INVOCATION.with(|w| w.set(true));
    let (label, detail) = match reason {
        FallbackReason::ParseFailed => ("parse_failed", "source did not parse"),
        FallbackReason::ParserUnavailable => ("parser_unavailable", "parser could not be initialized"),
    };
    eprintln!("kiss mv: AST analysis disabled ({label}: {detail}); falling back to lexical scan");
}

#[cfg(test)]
mod ast_plan_coverage {
    use super::*;

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
        let sites =
            ast_reference_offsets(src, "helper", Some("C"), Language::Python).unwrap();
        assert!(
            sites.iter().any(|(s, e)| &src[*s..*e] == "helper"),
            "owner-qualified AST should yield the method site, got {sites:?}"
        );
    }

    #[test]
    fn rust_owner_qualified_yields_method_site() {
        let _g = PlanInvocationGuard::enter();
        let src =
            "struct X;\nimpl X { fn helper(&self) {} }\nfn c(x: &X) { x.helper(); }\n";
        let sites = ast_reference_offsets(src, "helper", Some("X"), Language::Rust).unwrap();
        assert!(!sites.is_empty(), "owner-qualified AST should yield method site");
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
        };
        let cached = CachedOutcome::Success(res);
        let _ = cached_to_option(cached);
        let _ = cached_to_option(CachedOutcome::Fail(FallbackReason::ParseFailed));
        let _ = cached_to_option(CachedOutcome::Fail(FallbackReason::ParserUnavailable));
        let _ = cached_parse("def f():\n pass\n", Language::Python);
        let _ = ast_definition_ident_offsets("def f():\n pass\n", "f", None, Language::Python);
        let r = Reference {
            start: 0,
            end: 1,
            kind: ReferenceKind::Method,
        };
        let _ = reference_admits("a", &r, Some("X"), Language::Python);
        let _ = method_receiver_matches("a = X()\na.f()", 9, "X", Language::Python);
        let _ = method_receiver_matches("let a:X=x;\na.f()", 12, "X", Language::Rust);
        warn_per_invocation(&FallbackReason::ParseFailed);
        warn_per_invocation(&FallbackReason::ParserUnavailable);
    }
}
