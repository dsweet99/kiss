//! Receiver-type inference for owner-qualified `kiss mv` rename. Extracted
//! from `reference.rs` to keep that file under the `lines_per_file` gate.

use super::definition::{find_impl_blocks, find_python_class_block};
#[path = "reference_inference_assignments.rs"]
mod reference_inference_assignments;
use reference_inference_assignments::{
    tuple_assignment_receiver_type, type_from_assignment_rhs,
    type_from_assignment_target,
};

pub(super) fn infer_python_receiver_type(content: &str, receiver: &str) -> Option<String> {
    infer_python_receiver_type_at(content, content.len(), receiver)
}

pub(super) fn infer_python_receiver_type_at(
    content: &str,
    upto: usize,
    receiver: &str,
) -> Option<String> {
    let is_capitalized = {
        let first = receiver.trim_end_matches("()").chars().next();
        first.is_some_and(|c| c.is_ascii_uppercase())
    };
    if let Some((method, base)) = split_method_receiver(receiver) {
        let owner_hint = if base.is_empty() {
            None
        } else {
            let base_receiver = extract_receiver(&format!("{base}."));
            infer_python_receiver_type_at(content, upto, &base_receiver)
        };
        return python_method_return_type(content, upto, method, owner_hint.as_deref());
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
    if let Some(class_name) = class_or_capitalized {
        return Some(class_name);
    }
    if let Some(pos) = rfind_word_boundary(scope, &format!("{receiver} = "))
        && let Some(t) = type_from_assignment_target(scope, pos, receiver)
    {
        return Some(t);
    }
    if let Some(t) = tuple_assignment_receiver_type(scope, receiver) {
        return Some(t);
    }
    if let Some(pos) = rfind_word_boundary(scope, &format!("{receiver} := "))
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
    if let Some(inner) = after_head.strip_prefix('[') {
        let wrappers = [
            "Optional",
            "List",
            "Sequence",
            "Iterable",
            "Iterator",
            "Tuple",
            "Set",
            "FrozenSet",
            "Generator",
            "Awaitable",
            "Coroutine",
            "ClassVar",
            "Final",
            "Annotated",
            "Type",
            "Union",
            "Literal",
        ];
        if wrappers.contains(&last) {
            return unwrap_python_annotation(inner);
        }
    }
    if first_upper {
        Some(last.to_string())
    } else {
        None
    }
}

pub(super) fn extract_receiver(before: &str) -> String {
    let line = before.rsplit('\n').next().unwrap_or(before);
    let trimmed = line.trim_end_matches('.').trim_end();
    if let Some((base, name)) = split_trailing_method_call(trimmed)
        && !name.is_empty()
        && !name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
    {
        return format!("@method:{name}|{base}");
    }
    let was_method_call = before.trim_end_matches('.').ends_with("()");
    let trimmed = before.trim_end_matches('.').trim_end_matches("()");
    let start = trimmed
        .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .map_or(0, |idx| idx + 1);
    let name = &trimmed[start..];
    let is_constructor = name.chars().next().is_some_and(|c| c.is_ascii_uppercase());
    if was_method_call && !is_constructor {
        let base = trimmed[..start.saturating_sub(1)].trim_end();
        if base.is_empty() {
            format!("@method:{name}|")
        } else {
            format!("@method:{name}|{base}")
        }
    } else {
        name.to_string()
    }
}

fn split_trailing_method_call(text: &str) -> Option<(&str, &str)> {
    let close = text.rfind(')')?;
    let open = matching_open_paren(text, close)?;
    let head = &text[..open];
    let dot = head.rfind('.')?;
    let base = head[..dot].trim_end();
    let name = head[dot + 1..].trim_end();
    Some((base, name))
}

fn matching_open_paren(text: &str, close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, ch) in text[..=close].char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn infer_receiver_type(content: &str, receiver: &str) -> Option<String> {
    infer_receiver_type_at(content, content.len(), receiver)
}

pub(super) fn infer_receiver_type_at(content: &str, upto: usize, receiver: &str) -> Option<String> {
    let upto = upto.min(content.len());
    if let Some((method, base)) = split_method_receiver(receiver) {
        let owner_hint = if base.is_empty() {
            None
        } else {
            let base_receiver = extract_receiver(&format!("{base}."));
            infer_receiver_type_at(content, upto, &base_receiver)
        };
        return method_return_type(content, upto, method, owner_hint.as_deref());
    }
    [
        format!("let {receiver}: "),
        format!("let {receiver} : "),
        format!("{receiver}: "),
        format!("({receiver}: "),
        format!(", {receiver}: "),
    ]
    .into_iter()
    .find_map(|pat| type_after_pattern_last_before(content, upto, &pat))
}

pub(super) fn method_return_type(
    content: &str,
    upto: usize,
    method: &str,
    owner_hint: Option<&str>,
) -> Option<String> {
    let upto = upto.min(content.len());
    if let Some(type_name) = owner_hint {
        for (lo, hi) in find_impl_blocks(content, type_name).into_iter().rev() {
            let end = upto.min(hi);
            let scope = &content[lo..end];
            if let Some(pos) = find_last_rust_fn_def(scope, method) {
                return rust_method_return_type_from_pos(content, lo + pos);
            }
        }
    }
    let scope = &content[..upto];
    let pos = find_last_rust_fn_def(scope, method)?;
    rust_method_return_type_from_pos(content, pos)
}

fn find_last_rust_fn_def(scope: &str, method: &str) -> Option<usize> {
    rfind_word_boundary(scope, &format!("fn {method}("))
}

fn rust_method_return_type_from_pos(content: &str, pos: usize) -> Option<String> {
    let after_paren = content[pos..].find(')')? + pos + 1;
    let arrow_rest = content[after_paren..].trim_start();
    let arrow_rest = arrow_rest.strip_prefix("->")?.trim_start();
    let stripped = strip_rust_type_prefix(arrow_rest);
    let ty: String = stripped
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!ty.is_empty()).then_some(ty)
}

pub(super) fn type_after_pattern_last_before(
    content: &str,
    upto: usize,
    pat: &str,
) -> Option<String> {
    let pos = if pat
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        rfind_word_boundary(&content[..upto], pat)?
    } else {
        content[..upto].rfind(pat)?
    };
    let after = &content[pos + pat.len()..];
    let stripped = strip_rust_type_prefix(after);
    let ty: String = stripped
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!ty.is_empty()).then_some(ty)
}

pub(super) fn strip_rust_type_prefix(s: &str) -> &str {
    let mut s = s;
    loop {
        let trimmed = s.trim_start();
        if let Some(rest) = trimmed.strip_prefix('&') {
            s = rest;
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix("mut ")
            .or_else(|| trimmed.strip_prefix("dyn "))
            .or_else(|| trimmed.strip_prefix("impl "))
        {
            s = rest;
            continue;
        }
        return trimmed;
    }
}

#[cfg(test)]
#[path = "reference_inference_coverage.rs"] mod reference_inference_coverage;
