//! Auxiliary planner helpers split out of `ast_plan.rs` to keep that file
//! under the `lines_per_file` gate. Covers ambiguity detection, shadowing
//! ranges, owner-aware reference filtering, and the parse-failure warning
//! sink (one line per file per `kiss mv` invocation).

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::Language;

use super::super::ast_models::{
    AstResult, Definition, FallbackReason, ParseOutcome, Reference, ReferenceKind,
};
use super::super::reference::{
    associated_call_owner_matches_pub, extract_receiver_pub, infer_python_receiver_type_pub,
    infer_rust_receiver_type_pub,
};
use super::{cached_parse_outcome, owner_aliases};

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
        ParseOutcome::Success(result) => {
            let aliases = owner_aliases(&result, content, owner, language);
            result
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
                        return false;
                    };
                    !aliases.contains(&type_name)
                        && method_receiver_is_generic_parameter(content, r.start, &type_name)
                })
        }
        ParseOutcome::Fail(_) => false,
    }
}

fn method_receiver_is_generic_parameter(content: &str, start: usize, inferred: &str) -> bool {
    let method_sig = content[..start].rfind("fn ").and_then(|fn_pos| {
        content[fn_pos..]
            .find('{')
            .map(|brace_pos| &content[fn_pos..fn_pos + brace_pos])
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

pub(super) fn shadowed_reference_ranges(
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
) -> Option<&Definition> {
    let mut enclosing: Option<&Definition> = None;
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

pub(super) fn reference_is_shadowed(start: usize, shadowed_ranges: &[(usize, usize)]) -> bool {
    shadowed_ranges
        .iter()
        .any(|&(shadow_start, shadow_end)| start >= shadow_start && start < shadow_end)
}

pub(super) fn reference_admits(
    content: &str,
    r: &Reference,
    owner: Option<&str>,
    owner_aliases: Option<&HashSet<String>>,
    language: Language,
) -> bool {
    match (r.kind, owner) {
        (ReferenceKind::Call | ReferenceKind::Import, None) => true,
        (ReferenceKind::Call, Some(type_name)) => {
            if associated_call_owner_matches_pub(content, r.start, type_name) {
                return true;
            }
            owner_aliases.is_some_and(|set| {
                set.iter()
                    .any(|alias| associated_call_owner_matches_pub(content, r.start, alias))
            })
        }
        (ReferenceKind::Method, Some(type_name)) => {
            method_receiver_matches(content, r.start, type_name, owner_aliases, language)
        }
        _ => false,
    }
}

pub(super) fn method_receiver_matches(
    content: &str,
    start: usize,
    type_name: &str,
    owner_aliases: Option<&HashSet<String>>,
    language: Language,
) -> bool {
    let before = &content[..start];
    let receiver = extract_receiver_pub(before);
    let inferred = match language {
        Language::Python => infer_python_receiver_type_pub(content, start, &receiver),
        Language::Rust => infer_rust_receiver_type_pub(content, start, &receiver),
    };
    let Some(inferred) = inferred else {
        return false;
    };
    if inferred == type_name {
        return true;
    }
    owner_aliases.is_some_and(|set| set.contains(&inferred))
}

pub(super) fn matches_name(content: &str, start: usize, end: usize, name: &str) -> bool {
    end <= content.len() && &content[start..end] == name
}

thread_local! {
    static WARNED_FILES: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
}

pub(super) fn warned_files_clear() {
    WARNED_FILES.with(|warned| warned.borrow_mut().clear());
}

pub(super) fn warn_on_parse_failure(path: &Path, reason: &FallbackReason) {
    let emitted = WARNED_FILES.with(|warned| warned.borrow_mut().insert(path.to_path_buf()));
    if !emitted {
        return;
    }
    let (label, detail) = match reason {
        FallbackReason::ParseFailed => ("parse_failed", "source did not parse"),
        FallbackReason::ParserUnavailable => {
            ("parser_unavailable", "parser could not be initialized")
        }
    };
    eprintln!(
        "kiss mv: {path}: skipping file ({label}: {detail})",
        path = path.display()
    );
}
