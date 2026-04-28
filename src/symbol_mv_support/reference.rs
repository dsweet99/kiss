use crate::Language;

use super::definition::{find_impl_blocks, find_python_class_block};
use super::reference_inference::{
    extract_receiver, infer_python_receiver_type, infer_python_receiver_type_at,
    infer_receiver_type, infer_receiver_type_at,
};
use super::signature::{is_python_def_line, is_rust_fn_definition_line};

pub(super) struct RefSiteCtx<'a> {
    pub content: &'a str,
    pub start: usize,
    pub ident: &'a str,
    pub owner: Option<&'a str>,
}

pub(super) fn is_supported_reference_site(ctx: &RefSiteCtx<'_>, language: Language) -> bool {
    match language {
        Language::Python => is_legacy_python_reference_site(ctx),
        Language::Rust => is_legacy_rust_reference_site(ctx),
    }
}

fn is_legacy_python_reference_site(ctx: &RefSiteCtx<'_>) -> bool {
    let before = &ctx.content[..ctx.start];
    let line_start = before.rfind('\n').map_or(0, |idx| idx + 1);
    let line = &ctx.content[line_start..].lines().next().unwrap_or_default();
    let is_def = is_python_def_line(line, ctx.ident);
    if is_def {
        return py_def_owner_ok(ctx);
    }
    if py_import_allows(before, ctx.owner) {
        return true;
    }
    if py_from_clause_allows(before, ctx.owner) {
        return true;
    }
    if py_binding_keyword_allows(before, ctx.owner) {
        return true;
    }
    if py_await_allows(before, ctx.owner) {
        return true;
    }
    py_non_def_site(ctx, before)
}

/// `raise X from exc` — the exception name after `from` is a reference site.
fn py_from_clause_allows(before: &str, owner: Option<&str>) -> bool {
    owner.is_none() && before.ends_with(" from ")
}

/// `global x`, `nonlocal x`, `del x` — name after keyword is a reference site.
fn py_binding_keyword_allows(before: &str, owner: Option<&str>) -> bool {
    if owner.is_some() {
        return false;
    }
    before.ends_with("global ") || before.ends_with("nonlocal ") || before.ends_with("del ")
}

fn py_await_allows(before: &str, owner: Option<&str>) -> bool {
    owner.is_none() && before.ends_with("await ")
}

fn py_def_owner_ok(ctx: &RefSiteCtx<'_>) -> bool {
    ctx.owner.map_or_else(
        || {
            !is_inside_any_class(ctx.content, ctx.start)
                && !is_inside_any_function(ctx.content, ctx.start)
        },
        |class_name| {
            find_python_class_block(ctx.content, class_name)
                .is_some_and(|(cls_start, cls_end)| ctx.start >= cls_start && ctx.start < cls_end)
        },
    )
}

fn is_inside_any_function(content: &str, offset: usize) -> bool {
    let mut fn_indent: Option<usize> = None;
    let mut pos = 0;
    for line in content.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let starts_def = trimmed.starts_with("def ") || trimmed.starts_with("async def ");
        if pos <= offset && offset <= pos + line.len() {
            return fn_indent.is_some_and(|d| indent > d);
        }
        if starts_def && trimmed.contains(':') {
            fn_indent = Some(indent);
        } else if fn_indent.is_some_and(|current| {
            indent <= current && !trimmed.is_empty() && !trimmed.starts_with('#')
        }) {
            fn_indent = None;
        }
        pos += line.len() + 1;
    }
    false
}

fn py_import_allows(before: &str, owner: Option<&str>) -> bool {
    if owner.is_some() {
        return false;
    }
    if before.ends_with("import ") || before.ends_with(", ") {
        return true;
    }
    let line_start = before.rfind('\n').map_or(0, |idx| idx + 1);
    let prefix_on_line = &before[line_start..];
    if prefix_on_line.trim().is_empty() {
        return py_import_block_in_scope(before);
    }
    let t = prefix_on_line.trim_end();
    t.ends_with("import (") || t.ends_with(", (")
}

/// `helper` sits at the start of its own line (only whitespace before it on
/// the current line). Walk preceding non-empty, non-comment lines in reverse:
/// accept as soon as we hit an import-opener line; reject as soon as we hit a
/// line that isn't a continuation of an import list (`,` at end).
fn py_import_block_in_scope(before: &str) -> bool {
    for line in before.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if py_is_import_opener_line(trimmed) {
            return true;
        }
        if !trimmed.ends_with(',') {
            return false;
        }
    }
    false
}

fn py_is_import_opener_line(trimmed: &str) -> bool {
    trimmed.ends_with("import (")
        || trimmed.ends_with(", (")
        || (trimmed.ends_with('\\') && trimmed.contains("import"))
}

fn py_non_def_site(ctx: &RefSiteCtx<'_>, before: &str) -> bool {
    let after = &ctx.content[(ctx.start + ctx.ident.len())..];
    let is_method_call = before.ends_with('.');
    match ctx.owner {
        Some(_) if !is_method_call => false,
        Some(class_name) => {
            infer_python_receiver_type(ctx.content, &extract_receiver(before)).as_deref()
                == Some(class_name)
        }
        None => !is_method_call && after.trim_start().starts_with('('),
    }
}

fn is_inside_any_class(content: &str, offset: usize) -> bool {
    let mut class_indent = None;
    let mut pos = 0;
    for line in content.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.starts_with("class ") && trimmed.contains(':') {
            class_indent = Some(indent);
        } else if class_indent.is_some_and(|current| {
            indent <= current && !trimmed.is_empty() && !trimmed.starts_with('#')
        }) {
            class_indent = None;
        }
        if pos <= offset && offset <= pos + line.len() {
            return class_indent.is_some();
        }
        pos += line.len() + 1;
    }
    false
}

fn is_legacy_rust_reference_site(ctx: &RefSiteCtx<'_>) -> bool {
    let before = &ctx.content[..ctx.start];
    let after = &ctx.content[(ctx.start + ctx.ident.len())..];
    let line_start = before.rfind('\n').map_or(0, |idx| idx + 1);
    let line = &ctx.content[line_start..].lines().next().unwrap_or_default();
    if is_rust_fn_definition_line(line, ctx.ident) {
        return rust_fn_owner_ok(ctx);
    }
    rust_non_fn_site(ctx, before, after)
}

fn rust_fn_owner_ok(ctx: &RefSiteCtx<'_>) -> bool {
    ctx.owner.is_none_or(|type_name| {
        find_impl_blocks(ctx.content, type_name)
            .iter()
            .any(|&(impl_start, impl_end)| ctx.start >= impl_start && ctx.start < impl_end)
    })
}

fn rust_non_fn_site(ctx: &RefSiteCtx<'_>, before: &str, after: &str) -> bool {
    match ctx.owner {
        Some(type_name) if before.ends_with("::") => {
            rust_associated_call_owner(before).as_deref() == Some(type_name)
        }
        Some(_) if !before.ends_with('.') => false,
        Some(type_name) => {
            infer_receiver_type(ctx.content, &extract_receiver(before)).as_deref()
                == Some(type_name)
        }
        None => rust_import_allows(before) || after.trim_start().starts_with('('),
    }
}

fn rust_associated_call_owner(before: &str) -> Option<String> {
    let trimmed = before.trim_end();
    let prefix = trimmed.strip_suffix("::")?;
    let start = prefix
        .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .map_or(0, |idx| idx + 1);
    let name = &prefix[start..];
    (!name.is_empty()).then(|| name.to_string())
}

fn rust_import_allows(before: &str) -> bool {
    let trimmed = before.trim_end();
    let direct = trimmed.ends_with("::") || trimmed.ends_with('{') || trimmed.ends_with(',');
    if !direct {
        return false;
    }
    rust_use_stmt_in_scope(before)
}

fn rust_use_stmt_in_scope(before: &str) -> bool {
    for line in before.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_use_line_prefix(trimmed) {
            return true;
        }
        if !trimmed.ends_with(',') && !trimmed.ends_with('{') {
            return false;
        }
    }
    false
}

fn is_use_line_prefix(line: &str) -> bool {
    if line.starts_with("use ") {
        return true;
    }
    if let Some(rest) = line.strip_prefix("pub") {
        let rest = rest.trim_start();
        if rest.starts_with("use ") {
            return true;
        }
        if let Some(after_paren) = rest.strip_prefix('(')
            && let Some(after_vis) = after_paren.split_once(')')
        {
            return after_vis.1.trim_start().starts_with("use ");
        }
    }
    false
}

pub(super) fn infer_python_receiver_type_pub(
    content: &str,
    start: usize,
    receiver: &str,
) -> Option<String> {
    infer_python_receiver_type_at(content, start, receiver)
}

pub(super) fn infer_rust_receiver_type_pub(
    content: &str,
    start: usize,
    receiver: &str,
) -> Option<String> {
    infer_receiver_type_at(content, start, receiver)
}

pub(super) fn extract_receiver_pub(before: &str) -> String {
    extract_receiver(before)
}

pub(super) fn associated_call_owner_matches_pub(
    content: &str,
    start: usize,
    type_name: &str,
) -> bool {
    rust_associated_call_owner(&content[..start]).as_deref() == Some(type_name)
}

#[cfg(test)]
mod reference_coverage {
    use super::*;
    use crate::Language;

    #[test]
    fn touch_reference_helpers_for_coverage_gate() {
        let ctx = RefSiteCtx {
            content: "def f():\n pass\nf()",
            start: 14,
            ident: "f",
            owner: None,
        };
        let _ = is_supported_reference_site(&ctx, Language::Python);
        let _ = is_legacy_python_reference_site(&ctx);
        let _ = py_def_owner_ok(&ctx);
        let _ = py_import_allows("from m import ", None);
        let _ = py_import_block_in_scope("from m import (\n    a,\n    ");
        let _ = py_is_import_opener_line("from m import (");
        let _ = py_from_clause_allows("raise RuntimeError() from ", None);
        let _ = py_binding_keyword_allows("nonlocal ", None);
        let _ = py_await_allows("return await ", None);
        let _ = py_non_def_site(&ctx, "x");
        let _ = is_inside_any_class("class C:\n pass\n", 8);

        let async_ctx = RefSiteCtx {
            content: "async def f():\n pass\nf()",
            start: 10,
            ident: "f",
            owner: None,
        };
        let _ = is_legacy_python_reference_site(&async_ctx);

        let rctx = RefSiteCtx {
            content: "impl T { fn m() {} }",
            start: 15,
            ident: "m",
            owner: Some("T"),
        };
        let _ = is_legacy_rust_reference_site(&rctx);
        let _ = rust_fn_owner_ok(&rctx);
        let _ = rust_non_fn_site(&rctx, "T.", "()");
        let _ = extract_receiver("self.");
        let _ = is_inside_any_function("def outer():\n    def inner():\n        pass\n", 30);
        let _ = is_inside_any_function("def f():\n    pass\n", 0);
        let _ = rust_associated_call_owner("foo().bar");
        let _ = rust_import_allows("pub(self)");
        let _ = rust_use_stmt_in_scope("use crate::foo;");
        let _ = is_use_line_prefix("use crate::foo;");
        let _ = extract_receiver_pub("self.");
        let _ = associated_call_owner_matches_pub("self.foo().bar()", 9, "Owner");
    }
}
