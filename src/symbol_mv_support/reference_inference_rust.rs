//! Rust receiver-type / annotation helpers split out of
//! `reference_inference.rs` to keep that file under the `lines_per_file` gate.

use super::{find_impl_blocks, rfind_word_boundary, split_method_receiver};

pub(crate) fn extract_receiver(before: &str) -> String {
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

pub(crate) fn infer_receiver_type_at(content: &str, upto: usize, receiver: &str) -> Option<String> {
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
    if (receiver == "self" || receiver == "Self")
        && let Some(ty) = enclosing_rust_impl_type(content, upto)
    {
        return Some(ty);
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

/// Walk backwards from `upto` to the innermost enclosing `impl` block that
/// contains the offset, and return the *type* the impl is for. Recognizes
/// both `impl Type { ... }` and `impl Trait for Type { ... }` shapes.
/// `Self` and `self` inside such a block resolve to this type.
pub(crate) fn enclosing_rust_impl_type(content: &str, upto: usize) -> Option<String> {
    let bytes = content.as_bytes();
    let mut search_end = upto;
    while let Some(impl_pos) = content[..search_end].rfind("impl") {
        search_end = impl_pos;
        let prev_ok = impl_pos == 0
            || !matches!(bytes[impl_pos - 1], b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9');
        let after_impl_idx = impl_pos + "impl".len();
        let next_ok = after_impl_idx >= bytes.len()
            || !matches!(bytes[after_impl_idx], b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9');
        if !(prev_ok && next_ok) {
            continue;
        }
        let Some(brace_rel) = content[after_impl_idx..upto.max(after_impl_idx)].find('{') else {
            continue;
        };
        let brace = after_impl_idx + brace_rel;
        if let Some(end) = block_end(content, brace)
            && upto >= brace
            && upto < end
            && let Some(ty) = parse_impl_target(&content[after_impl_idx..brace])
        {
            return Some(ty);
        }
    }
    None
}

/// Given the header text between `impl` and the opening brace, return the
/// concrete type the impl applies to. Strips a leading generic param list
/// (`<T: Bound>`), handles the `Trait for Type` shape, and unwraps a single
/// generic argument list on the type itself (`Foo<T> -> Foo`).
fn parse_impl_target(header: &str) -> Option<String> {
    let mut s = header.trim_start();
    if s.starts_with('<') {
        let mut depth = 0i32;
        let mut end = None;
        for (idx, ch) in s.char_indices() {
            match ch {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(idx + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        s = &s[end?..];
        s = s.trim_start();
    }
    let target = s.rsplit(" for ").next().unwrap_or(s);
    let bare: String = target
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!bare.is_empty()).then_some(bare)
}

fn block_end(content: &str, open_brace: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut depth = 0i32;
    let mut idx = open_brace;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(idx + 1);
                }
            }
            _ => {}
        }
        idx += 1;
    }
    None
}

pub(crate) fn method_return_type(
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

pub(crate) fn type_after_pattern_last_before(
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

pub(crate) fn strip_rust_type_prefix(s: &str) -> &str {
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

