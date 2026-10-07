use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::graph::{extract_dynamic_import_module, is_dunder_import, is_importlib_import_module};
use crate::parsing::ParsedFile;

use super::python_path::is_python_test_module_path;
use super::types::CodeContextSet;

enum DynamicBounds {
    Unbounded,
    Shapes(Vec<Vec<Frag>>),
}

enum Bound {
    Unbounded,
    Shaped(Vec<Frag>),
}

enum Frag {
    Lit(String),
    Hole,
}

enum FStringImport {
    Concrete,
    Shaped(Vec<Frag>),
    Unbounded,
}

pub(super) fn apply_dynamic_import_promotion(
    contexts: &mut HashMap<PathBuf, CodeContextSet>,
    parsed: &[&ParsedFile],
    discovered: &[PathBuf],
) {
    match production_dynamic_bounds(parsed) {
        DynamicBounds::Unbounded => promote_test_named(contexts, parsed, discovered, |_| true),
        DynamicBounds::Shapes(shapes) if shapes.is_empty() => {}
        DynamicBounds::Shapes(shapes) => {
            promote_test_named(contexts, parsed, discovered, |path| {
                shapes.iter().any(|shape| path_matches_shape(path, shape))
            });
        }
    }
}

fn production_dynamic_bounds(parsed: &[&ParsedFile]) -> DynamicBounds {
    let mut shapes = Vec::new();
    for file in parsed {
        if is_python_test_module_path(&file.path) {
            continue;
        }
        for bound in bounds_in_file(file) {
            match bound {
                Bound::Unbounded => return DynamicBounds::Unbounded,
                Bound::Shaped(shape) => shapes.push(shape),
            }
        }
    }
    DynamicBounds::Shapes(shapes)
}

fn bounds_in_file(file: &ParsedFile) -> Vec<Bound> {
    if !file.source.contains("import_module") && !file.source.contains("__import__") {
        return Vec::new();
    }
    let mut out = Vec::new();
    walk_bounds(file.tree.root_node(), &file.source, &mut out);
    out
}

fn walk_bounds(node: tree_sitter::Node<'_>, source: &str, out: &mut Vec<Bound>) {
    if node.kind() == "call" {
        note_call(node, source, out);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_bounds(child, source, out);
    }
}

fn note_call(node: tree_sitter::Node<'_>, source: &str, out: &mut Vec<Bound>) {
    let Some(func) = node.child_by_field_name("function") else {
        return;
    };
    let dynamic = is_importlib_import_module(func, source) || is_dunder_import(func, source);
    if !dynamic || extract_dynamic_import_module(node, source).is_some() {
        return;
    }
    let Some(arg) = module_arg(node) else {
        out.push(Bound::Unbounded);
        return;
    };
    if arg.kind() == "string" {
        match fstring_import(arg, source) {
            FStringImport::Concrete => return,
            FStringImport::Shaped(shape) => {
                out.push(Bound::Shaped(shape));
                return;
            }
            FStringImport::Unbounded => {}
        }
    }
    out.push(Bound::Unbounded);
}

fn module_arg(call: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let args = call.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    args.children(&mut cursor).find(|child| {
        !matches!(
            child.kind(),
            "," | "(" | ")" | "comment" | "keyword_argument"
        )
    })
}

fn fstring_import(node: tree_sitter::Node<'_>, source: &str) -> FStringImport {
    if !string_start_is_f(node, source) {
        return FStringImport::Unbounded;
    }
    let mut frags = Vec::new();
    let mut saw_hole = false;
    let mut literal = String::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "string_start" | "string_end" => {}
            "interpolation" => {
                saw_hole = true;
                push_lit(&mut frags, &mut literal);
                frags.push(Frag::Hole);
            }
            "string_content" => {
                let Ok(text) = child.utf8_text(source.as_bytes()) else {
                    return FStringImport::Unbounded;
                };
                literal.push_str(text);
            }
            _ => return FStringImport::Unbounded,
        }
    }
    push_lit(&mut frags, &mut literal);
    if !saw_hole {
        return if frags.iter().any(|frag| matches!(frag, Frag::Lit(_))) {
            FStringImport::Concrete
        } else {
            FStringImport::Unbounded
        };
    }
    if frags.iter().any(|frag| matches!(frag, Frag::Lit(_))) {
        FStringImport::Shaped(frags)
    } else {
        FStringImport::Unbounded
    }
}

fn push_lit(frags: &mut Vec<Frag>, literal: &mut String) {
    if !literal.is_empty() {
        frags.push(Frag::Lit(std::mem::take(literal)));
    }
}

fn string_start_is_f(node: tree_sitter::Node<'_>, source: &str) -> bool {
    node.child(0)
        .filter(|child| child.kind() == "string_start")
        .and_then(|child| child.utf8_text(source.as_bytes()).ok())
        .is_some_and(|text| text.contains('f') || text.contains('F'))
}

fn path_matches_shape(path: &Path, frags: &[Frag]) -> bool {
    module_suffixes(path)
        .iter()
        .any(|module| shape_matches(module, frags))
}

fn module_suffixes(path: &Path) -> Vec<String> {
    let mut comps: Vec<String> = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(str::to_string)
        .collect();
    if let Some(file) = comps.last_mut()
        && let Some(stem) = Path::new(file.as_str()).file_stem()
    {
        *file = stem.to_string_lossy().into_owned();
    }
    (0..comps.len())
        .map(|start| comps[start..].join("."))
        .collect()
}

fn shape_matches(module: &str, frags: &[Frag]) -> bool {
    match_frags(module, frags)
}

fn match_frags(rest: &str, frags: &[Frag]) -> bool {
    let Some((first, tail)) = frags.split_first() else {
        return rest.is_empty();
    };
    match first {
        Frag::Lit(lit) => rest.starts_with(lit.as_str()) && match_frags(&rest[lit.len()..], tail),
        Frag::Hole => match_hole(rest, tail),
    }
}

fn match_hole(rest: &str, tail: &[Frag]) -> bool {
    let Some(len) = ident_prefix_len(rest) else {
        return false;
    };
    match_frags(&rest[len..], tail)
}

fn ident_prefix_len(text: &str) -> Option<usize> {
    let mut chars = text.chars();
    let first = chars.next()?;
    if first != '_' && !first.is_ascii_alphabetic() {
        return None;
    }
    let mut len = first.len_utf8();
    for ch in chars {
        if ch != '_' && !ch.is_ascii_alphanumeric() {
            break;
        }
        len += ch.len_utf8();
    }
    Some(len)
}

fn promote_test_named(
    contexts: &mut HashMap<PathBuf, CodeContextSet>,
    parsed: &[&ParsedFile],
    discovered: &[PathBuf],
    pred: impl Fn(&Path) -> bool,
) {
    for path in parsed
        .iter()
        .map(|file| file.path.as_path())
        .chain(discovered.iter().map(PathBuf::as_path))
    {
        if is_python_test_module_path(path)
            && pred(path)
            && let Some(ctx) = contexts.get_mut(&crate::rust_include::canonical_path(path))
        {
            ctx.production = true;
        }
    }
}
