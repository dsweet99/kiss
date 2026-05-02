use super::definition::{find_impl_blocks, find_python_class_block};

#[path = "reference_inference_assignments.rs"]
mod reference_inference_assignments;
#[path = "reference_inference_rust.rs"]
mod reference_inference_rust;

#[cfg(test)]
#[path = "reference_inference_coverage.rs"]
mod reference_inference_coverage;

#[cfg(test)]
pub(super) use reference_inference_assignments::is_tuple_assignment_at;
pub(super) use reference_inference_assignments::{
    tuple_assignment_receiver_type, type_from_assignment_rhs, type_from_assignment_target,
};
pub(super) use reference_inference_rust::{
    enclosing_rust_impl_type, extract_receiver, infer_receiver_type_at,
};
#[cfg(test)]
pub(super) use reference_inference_rust::{
    method_return_type, strip_rust_type_prefix, type_after_pattern_last_before,
};

// Receiver-type inference for owner-qualified `kiss mv` rename. Extracted
// from `reference.rs` to keep that file under the `lines_per_file` gate.

pub(super) fn infer_python_receiver_type_at(
    content: &str,
    upto: usize,
    receiver: &str,
) -> Option<String> {
    let is_capitalized = {
        let first = receiver.trim_end_matches("()").chars().next();
        first.is_some_and(|c| c.is_ascii_uppercase())
    };
    let method_receiver = split_method_receiver(receiver);
    if matches!(method_receiver, Some((method, _)) if method == "super") {
        return enclosing_python_class(content, upto)
            .and_then(|cls| python_class_first_base(content, &cls));
    }
    if let Some((method, base)) = method_receiver {
        let owner_hint = if base.is_empty() {
            None
        } else {
            let base_receiver = extract_receiver(&format!("{base}."));
            infer_python_receiver_type_at(content, upto, &base_receiver)
        };
        if let Some(ty) = python_method_return_type(content, upto, method, owner_hint.as_deref()) {
            return Some(ty);
        }
    }
    let receiver = receiver.trim_end_matches("()");
    let upto = upto.min(content.len());
    let scope = enclosing_python_function_slice(content, upto).unwrap_or(&content[..upto]);
    let class_or_capitalized = if receiver == "self" || receiver == "cls" {
        enclosing_python_class(content, upto)
    } else if is_capitalized {
        Some(receiver.to_string())
    } else {
        None
    };
    fallback_python_receiver_type(scope, receiver, class_or_capitalized)
}

fn fallback_python_receiver_type(
    scope: &str,
    receiver: &str,
    class_or_capitalized: Option<String>,
) -> Option<String> {
    if let Some(class_name) = class_or_capitalized {
        return Some(class_name);
    }
    let assign_pat = format!("{receiver} = ");
    if let Some(pos) = rfind_word_boundary(scope, &assign_pat)
        && let Some(t) = type_from_assignment_target(scope, pos, receiver)
    {
        return Some(t);
    }
    if let Some(t) = tuple_assignment_receiver_type(scope, receiver) {
        return Some(t);
    }
    let walrus_pat = format!("{receiver} := ");
    if let Some(pos) = rfind_word_boundary(scope, &walrus_pat)
        && let Some(t) = type_from_assignment_rhs(&scope[pos + receiver.len() + 4..])
    {
        return Some(t);
    }
    type_from_python_param_annotation(scope, receiver)
}

pub(super) fn rfind_word_boundary(haystack: &str, pat: &str) -> Option<usize> {
    let mut search_end = haystack.len();
    while let Some(pos) = haystack[..search_end].rfind(pat) {
        let prev_byte = if pos == 0 {
            None
        } else {
            Some(haystack.as_bytes()[pos - 1])
        };
        let is_boundary = prev_byte.is_none_or(|b| !b.is_ascii_alphanumeric() && b != b'_');
        if is_boundary {
            return Some(pos);
        }
        if pos == 0 {
            return None;
        }
        search_end = pos;
    }
    None
}

/// Return the first (leftmost) base-class name in the `class Foo(Bar, Baz):`
/// header for `class_name`, or `None` if the class declaration has no
/// parenthesized bases.
fn python_class_first_base(content: &str, class_name: &str) -> Option<String> {
    let needle = format!("class {class_name}");
    let pos = content.find(&needle)?;
    let after = &content[pos + needle.len()..];
    let after = after.strip_prefix('(')?;
    let close = after.find([')', ':'])?;
    let bases = &after[..close];
    let first = bases.split(',').next()?.trim();
    let bare: String = first
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!bare.is_empty()).then_some(bare)
}

/// Walk Python `class X(...)` declarations and return every X whose base
/// list mentions `parent` (transitively closed). Used to propagate a
/// parent-class method rename to subclass overrides — see KPOP H1.
pub(super) fn python_subclasses_of(
    content: &str,
    parent: &str,
) -> std::collections::HashSet<String> {
    use std::collections::HashSet;
    let direct_children = |class: &str| direct_python_subclasses_of(content, class);
    let mut acc: HashSet<String> = HashSet::new();
    let mut frontier = vec![parent.to_string()];
    while let Some(cur) = frontier.pop() {
        for child in direct_children(&cur) {
            if acc.insert(child.clone()) {
                frontier.push(child);
            }
        }
    }
    acc
}

fn direct_python_subclasses_of(content: &str, class: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("class ") else {
            continue;
        };
        let name_end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        let child_name = &rest[..name_end];
        let after_name = &rest[name_end..];
        let Some(inside) = after_name.strip_prefix('(') else {
            continue;
        };
        let close = inside.find(')').unwrap_or(inside.len());
        let bases_part = &inside[..close];
        let mentions = bases_part.split(',').any(|b| {
            let bare: String = b
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            bare.rsplit('.').next() == Some(class)
        });
        if mentions {
            out.push(child_name.to_string());
        }
    }
    out
}

pub(super) fn enclosing_python_class(content: &str, offset: usize) -> Option<String> {
    let mut class: Option<(usize, String)> = None;
    let mut pos = 0;
    for line in content.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if pos <= offset && offset <= pos + line.len() {
            return class.map(|(_, name)| name);
        }
        if let Some(rest) = trimmed.strip_prefix("class ")
            && let Some(name_end) = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        {
            class = Some((indent, rest[..name_end].to_string()));
        } else if class
            .as_ref()
            .is_some_and(|(d, _)| indent <= *d && !trimmed.is_empty() && !trimmed.starts_with('#'))
        {
            class = None;
        }
        pos += line.len() + 1;
    }
    None
}

pub(super) fn enclosing_python_function_slice(content: &str, offset: usize) -> Option<&str> {
    let mut fn_start: Option<(usize, usize)> = None;
    let mut pos = 0;
    for line in content.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if pos <= offset && offset <= pos + line.len() {
            return fn_start.map(|(start, _)| &content[start..offset]);
        }
        let starts_def = trimmed.starts_with("def ") || trimmed.starts_with("async def ");
        if starts_def && trimmed.contains(':') {
            fn_start = Some((pos, indent));
        } else if fn_start
            .is_some_and(|(_, d)| indent <= d && !trimmed.is_empty() && !trimmed.starts_with('#'))
        {
            fn_start = None;
        }
        pos += line.len() + 1;
    }
    None
}

pub(super) fn type_from_python_param_annotation(scope: &str, receiver: &str) -> Option<String> {
    for pat in [
        format!("({receiver}: "),
        format!(", {receiver}: "),
        format!("(\n    {receiver}: "),
    ] {
        if let Some(pos) = scope.rfind(&pat) {
            let rest = &scope[pos + pat.len()..];
            if let Some(name) = unwrap_python_annotation(rest) {
                return Some(name);
            }
        }
    }
    None
}

fn split_method_receiver(receiver: &str) -> Option<(&str, &str)> {
    let method = receiver.strip_prefix("@method:")?;
    let (method, base) = method.split_once('|').unwrap_or((method, ""));
    Some((method, base))
}

pub(super) fn python_method_return_type(
    content: &str,
    upto: usize,
    method: &str,
    owner_hint: Option<&str>,
) -> Option<String> {
    let upto = upto.min(content.len());
    if let Some(class_name) = owner_hint
        && let Some((cls_start, cls_end)) = find_python_class_block(content, class_name)
    {
        let end = upto.min(cls_end);
        if let Some(pos) = find_last_python_method_def(&content[cls_start..end], method) {
            return python_method_return_type_from_pos(content, cls_start + pos);
        }
    }
    let slice = &content[..upto];
    let pos = find_last_python_method_def(slice, method)?;
    python_method_return_type_from_pos(content, pos)
}

fn find_last_python_method_def(scope: &str, method: &str) -> Option<usize> {
    let async_pos = rfind_word_boundary(scope, &format!("async def {method}("));
    let sync_pos = rfind_word_boundary(scope, &format!("def {method}("));
    async_pos.into_iter().chain(sync_pos).max()
}

fn python_method_return_type_from_pos(content: &str, pos: usize) -> Option<String> {
    let after_paren = content[pos..].find(')')? + pos + 1;
    let rest = content[after_paren..].trim_start();
    let rest = rest.strip_prefix("->")?.trim_start();
    unwrap_python_annotation(rest)
}

pub(super) fn unwrap_python_annotation(rest: &str) -> Option<String> {
    let rest = rest.trim_start();
    if let Some(stripped) = python_quoted_annotation(rest) {
        return unwrap_python_annotation(stripped);
    }
    let head: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        .collect();
    let last = head.rsplit('.').next()?;
    if last.is_empty() {
        return None;
    }
    let first_upper = last.chars().next().is_some_and(|c| c.is_ascii_uppercase());
    let after_head = &rest[head.len()..];
    if let Some(inner) = after_head.strip_prefix('[')
        && is_python_wrapper_type(last)
    {
        return unwrap_python_annotation(inner);
    }
    if first_upper {
        Some(last.to_string())
    } else {
        None
    }
}

fn is_python_wrapper_type(ty: &str) -> bool {
    matches!(
        ty,
        "Optional"
            | "List"
            | "Sequence"
            | "Iterable"
            | "Iterator"
            | "Tuple"
            | "Set"
            | "FrozenSet"
            | "Generator"
            | "Awaitable"
            | "Coroutine"
            | "ClassVar"
            | "Final"
            | "Annotated"
            | "Type"
            | "Union"
            | "Literal"
    )
}

fn python_quoted_annotation(rest: &str) -> Option<&str> {
    if let Some(pos) = rest.strip_prefix('"') {
        let end = pos.find('"')?;
        return Some(&pos[..end]);
    }
    if let Some(pos) = rest.strip_prefix('\'') {
        let end = pos.find('\'')?;
        return Some(&pos[..end]);
    }
    None
}
