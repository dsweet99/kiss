//! AST-first planning helpers (Tasks 4 & 5).
//!
//! Call sites prefer the AST path; on parse failure they fall back to the
//! lexical helpers in `lex.rs` / `signature.rs` and emit a single-shape
//! warning so the fallback is observable. Returned offsets are byte offsets
//! into the original source string and may be unioned with lexical hits to
//! preserve current behavior during the transition.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::Language;

use super::ast_models::{AstResult, FallbackReason, ParseOutcome, Reference, ReferenceKind};
use super::ast_python::parse_python;
use super::ast_rust::parse_rust;
use super::reference::{
    associated_call_owner_matches_pub, extract_receiver_pub, infer_python_receiver_type_pub,
    infer_rust_receiver_type_pub,
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
        WARNED_FILES.with(|warned| warned.borrow_mut().clear());
        PARSE_CACHE.with(|c| c.borrow_mut().clear());
        Self
    }
}

impl Drop for PlanInvocationGuard {
    fn drop(&mut self) {
        WARNED_FILES.with(|warned| warned.borrow_mut().clear());
        PARSE_CACHE.with(|c| c.borrow_mut().clear());
    }
}

pub(super) fn cached_parse_outcome(content: &str, path: &Path, language: Language) -> ParseOutcome {
    let key = (content_hash(content), content.len(), lang_key(language));
    let cached = PARSE_CACHE.with(|c| c.borrow().get(&key).cloned());
    if let Some(hit) = cached {
        let outcome = cached_to_outcome(hit);
        if let ParseOutcome::Fail(reason) = &outcome {
            warn_on_parse_failure(path, reason);
        }
        return outcome;
    }
    let outcome = match parse_for(content, language) {
        ParseOutcome::Success(res) => CachedOutcome::Success(res),
        ParseOutcome::Fail(reason) => CachedOutcome::Fail(reason),
    };
    PARSE_CACHE.with(|c| c.borrow_mut().insert(key, outcome.clone()));
    let outcome = cached_to_outcome(outcome);
    if let ParseOutcome::Fail(reason) = &outcome {
        warn_on_parse_failure(path, reason);
    }
    outcome
}

#[cfg(test)]
fn cached_parse(content: &str, language: Language) -> Option<AstResult> {
    match cached_parse_outcome(content, Path::new("<test>"), language) {
        ParseOutcome::Success(res) => Some(res),
        ParseOutcome::Fail(_) => None,
    }
}

fn cached_to_outcome(outcome: CachedOutcome) -> ParseOutcome {
    match outcome {
        CachedOutcome::Success(res) => ParseOutcome::Success(res),
        CachedOutcome::Fail(reason) => ParseOutcome::Fail(reason),
    }
}

#[cfg(test)]
pub(super) fn ast_definition_span(
    content: &str,
    name: &str,
    owner: Option<&str>,
    language: Language,
) -> Option<(usize, usize)> {
    let result = cached_parse(content, language)?;
    ast_definition_span_from_result(&result, name, owner)
}

pub(super) fn ast_definition_span_from_result(
    result: &AstResult,
    name: &str,
    owner: Option<&str>,
) -> Option<(usize, usize)> {
    let def = result.matching_definition(name, owner)?;
    Some((def.start, def.end))
}

#[cfg(test)]
pub(super) fn ast_definition_ident_offsets(
    content: &str,
    name: &str,
    owner: Option<&str>,
    language: Language,
) -> Option<Vec<(usize, usize)>> {
    let result = cached_parse(content, language)?;
    Some(ast_definition_ident_offsets_from_result(
        &result, content, name, owner,
    ))
}

pub(super) fn ast_definition_ident_offsets_from_result(
    result: &AstResult,
    content: &str,
    name: &str,
    owner: Option<&str>,
) -> Vec<(usize, usize)> {
    let Some(def) = result.matching_definition(name, owner) else {
        return Vec::new();
    };
    let (s, e) = (def.name_start, def.name_end);
    assert!(
        e <= content.len() && &content[s..e] == name,
        "AST name span must match the symbol name exactly"
    );
    vec![(s, e)]
}

#[cfg(test)]
pub(super) fn ast_reference_offsets(
    content: &str,
    name: &str,
    owner: Option<&str>,
    language: Language,
) -> Option<Vec<(usize, usize)>> {
    let result = cached_parse(content, language)?;
    Some(ast_reference_offsets_from_result(
        &result, content, name, owner, language,
    ))
}

pub(super) fn ast_reference_offsets_raw_from_result(
    result: &AstResult,
    content: &str,
    name: &str,
    owner: Option<&str>,
    language: Language,
) -> Vec<(usize, usize)> {
    let mut sites = Vec::new();
    for r in &result.references {
        if !matches_name(content, r.start, r.end, name) {
            continue;
        }
        if !reference_admits(content, r, owner, language) {
            continue;
        }
        sites.push((r.start, r.end));
    }
    sites.sort_unstable();
    sites.dedup();
    sites
}

pub(super) fn ast_reference_offsets_from_result(
    result: &AstResult,
    content: &str,
    name: &str,
    owner: Option<&str>,
    language: Language,
) -> Vec<(usize, usize)> {
    let shadowed_ranges = shadowed_reference_ranges(result, name, owner);
    let mut sites = ast_reference_offsets_raw_from_result(result, content, name, owner, language);
    sites.retain(|(start, _)| !reference_is_shadowed(*start, &shadowed_ranges));
    sites.sort_unstable();
    sites.dedup();
    sites
}

pub(crate) fn has_ambiguous_method_reference(
    path: &Path,
    content: &str,
    name: &str,
    owner: Option<&str>,
    language: Language,
) -> bool {
    let Some(owner) = owner else {
        return false;
    };
    if !matches!(language, Language::Rust) {
        return false;
    }
    if content.contains(&format!("trait {owner}")) {
        return false;
    }
    match cached_parse_outcome(content, path, language) {
        ParseOutcome::Success(result) => result
            .references
            .iter()
            .filter(|r| matches_name(content, r.start, r.end, name))
            .filter(|r| matches!(r.kind, ReferenceKind::Method))
            .filter(|r| content[..r.start].ends_with('.'))
            .any(|r| {
                let inferred = infer_rust_receiver_type_pub(
                    content,
                    r.start,
                    &extract_receiver_pub(&content[..r.start]),
                );
                let Some(type_name) = inferred else {
                    return true;
                };
                type_name != owner
                    && method_receiver_is_generic_parameter(content, r.start, &type_name)
            }),
        ParseOutcome::Fail(_) => false,
    }
}

fn method_receiver_is_generic_parameter(
    content: &str,
    start: usize,
    inferred: &str,
) -> bool {
    let method_sig = content[..start].rfind("fn ").and_then(|fn_pos| {
        content[fn_pos..].find('{').map(|brace_pos| &content[fn_pos..fn_pos + brace_pos])
    });
    let Some(sig) = method_sig else {
        return false;
    };
    let Some(generic_start) = sig.find('<') else {
        return false;
    };
    let mut depth = 0usize;
    let mut generic_end = None;
    for (offset, ch) in sig[generic_start + 1..].char_indices() {
        match ch {
            '<' => depth = depth.saturating_add(1),
            '>' if depth == 0 => {
                generic_end = Some(offset);
                break;
            }
            '>' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let generic_end = match generic_end {
        Some(end) => generic_start + 1 + end,
        None => return false,
    };
    let generic_list = &sig[generic_start + 1..generic_end];
    generic_list
        .split(',')
        .filter_map(|part| part.split_whitespace().next())
        .any(|bound| {
            bound
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .eq(inferred)
        })
}

fn shadowed_reference_ranges(
    result: &AstResult,
    name: &str,
    owner: Option<&str>,
) -> Vec<(usize, usize)> {
    let Some(selected_def) = result.matching_definition(name, owner) else {
        return Vec::new();
    };
    let start_from_enclosing_scope = matches!(selected_def.language, Language::Rust);
    let mut ranges = Vec::new();
    for shadowed in &result.definitions {
        if shadowed.name != name
            || shadowed.owner.as_deref() != owner
            || (shadowed.start, shadowed.end) == (selected_def.start, selected_def.end)
        {
            continue;
        }
        if let Some(enclosing) = smallest_enclosing_definition(result, shadowed.start, shadowed.end)
        {
            let start = if start_from_enclosing_scope {
                enclosing.start
            } else {
                shadowed.start
            };
            ranges.push((start, enclosing.end));
        }
    }
    ranges
}

fn smallest_enclosing_definition(
    result: &AstResult,
    start: usize,
    end: usize,
) -> Option<&super::ast_models::Definition> {
    let mut enclosing: Option<&super::ast_models::Definition> = None;
    for candidate in &result.definitions {
        if (candidate.start, candidate.end) == (start, end) {
            continue;
        }
        if candidate.start <= start && candidate.end >= end {
            enclosing = match enclosing {
                Some(current) if current.end - current.start <= candidate.end - candidate.start => {
                    Some(current)
                }
                _ => Some(candidate),
            };
        }
    }
    enclosing
}

fn reference_is_shadowed(start: usize, shadowed_ranges: &[(usize, usize)]) -> bool {
    shadowed_ranges
        .iter()
        .any(|&(shadow_start, shadow_end)| start >= shadow_start && start < shadow_end)
}

fn reference_admits(content: &str, r: &Reference, owner: Option<&str>, language: Language) -> bool {
    match (r.kind, owner) {
        (ReferenceKind::Call | ReferenceKind::Import, None) => true,
        (ReferenceKind::Call, Some(type_name)) => {
            associated_call_owner_matches_pub(content, r.start, type_name)
        }
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
    static WARNED_FILES: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
}

fn warn_on_parse_failure(path: &Path, reason: &FallbackReason) {
    let emitted = WARNED_FILES.with(|warned| warned.borrow_mut().insert(path.to_path_buf()));
    if !emitted { return; }
    let (label, detail) = match reason {
        FallbackReason::ParseFailed => ("parse_failed", "source did not parse"),
        FallbackReason::ParserUnavailable => {
            ("parser_unavailable", "parser could not be initialized")
        }
    };
    eprintln!(
        "kiss mv: {path}: AST analysis disabled ({label}: {detail}); falling back to lexical scan",
        path = path.display()
    );
}

#[cfg(test)] #[path = "ast_plan_coverage.rs"] mod ast_plan_coverage;
