#[cfg(test)]
#[path = "ast_plan_coverage.rs"]
mod ast_plan_coverage;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use crate::Language;

use super::ast_models::{AstResult, FallbackReason, ParseOutcome};

fn owner_aliases(
    result: &AstResult,
    content: &str,
    owner: &str,
    language: Language,
) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    set.insert(owner.to_string());
    match language {
        Language::Rust => {
            for ti in &result.trait_impls {
                if ti.trait_name == owner {
                    set.insert(ti.implementor.clone());
                }
            }
        }
        Language::Python => {
            for sub in python_subclasses_of_pub(content, owner) {
                set.insert(sub);
            }
        }
    }
    set
}
use super::ast_python::parse_python;
use super::ast_rust::parse_rust;
use super::reference::python_subclasses_of_pub;

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
        warned_files_clear();
        PARSE_CACHE.with(|c| c.borrow_mut().clear());
        Self
    }
}

impl Drop for PlanInvocationGuard {
    fn drop(&mut self) {
        warned_files_clear();
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
        &result, content, name, owner, language,
    ))
}

pub(super) fn ast_definition_ident_offsets_from_result(
    result: &AstResult,
    content: &str,
    name: &str,
    owner: Option<&str>,
    language: Language,
) -> Vec<(usize, usize)> {
    if owner.is_none() {
        let Some(def) = result.matching_definition(name, owner) else {
            return Vec::new();
        };
        let (s, e) = (def.name_start, def.name_end);
        assert!(
            e <= content.len() && &content[s..e] == name,
            "AST name span must match the symbol name exactly"
        );
        return vec![(s, e)];
    }
    let aliases = owner.map(|o| owner_aliases(result, content, o, language));
    let mut spans = Vec::new();
    for def in &result.definitions {
        if def.name != name {
            continue;
        }
        let owner_matches = match (owner, def.owner.as_deref(), aliases.as_ref()) {
            (Some(_), Some(d_owner), Some(set)) if set.contains(d_owner) => true,
            (Some(o), Some(d_owner), _) => o == d_owner,
            _ => false,
        };
        if !owner_matches {
            continue;
        }
        def.assert_consistent();
        let (s, e) = (def.name_start, def.name_end);
        if e <= content.len() && &content[s..e] == name {
            spans.push((s, e));
        }
    }
    spans.sort_unstable();
    spans.dedup();
    spans
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
    let owner_set = owner.map(|o| owner_aliases(result, content, o, language));
    let mut sites = Vec::new();
    for r in &result.references {
        if !matches_name(content, r.start, r.end, name) {
            continue;
        }
        if !reference_admits(content, r, owner, owner_set.as_ref(), language) {
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

#[path = "ast_plan_extras.rs"]
mod ast_plan_extras;
pub(crate) use ast_plan_extras::has_ambiguous_method_reference;
use ast_plan_extras::{
    matches_name, reference_admits, reference_is_shadowed, shadowed_reference_ranges,
    warn_on_parse_failure, warned_files_clear,
};
