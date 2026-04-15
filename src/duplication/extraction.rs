use crate::minhash::normalize_code;
use crate::parsing::ParsedFile;
use crate::rust_parsing::ParsedRustFile;
use crate::units::get_child_by_field;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use syn::{ImplItem, Item};
use tree_sitter::Node;

const MIN_CHUNK_TOKENS: usize = 10;
const MIN_CHUNK_LINES: usize = 5;

#[derive(Debug, Clone)]
pub struct CodeChunk {
    pub file: PathBuf,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub normalized: String,
}

pub(crate) fn is_nontrivial_chunk(normalized: &str, line_count: usize) -> bool {
    line_count >= MIN_CHUNK_LINES && normalized.split_whitespace().count() >= MIN_CHUNK_TOKENS
}

#[must_use]
pub fn extract_chunks_for_duplication(parsed_files: &[&ParsedFile]) -> Vec<CodeChunk> {
    // Parallelize per-file extraction but preserve deterministic ordering:
    // - files in input order
    // - within each file, traversal order from `extract_function_chunks`
    let mut per_file: Vec<(usize, Vec<CodeChunk>)> = parsed_files
        .par_iter()
        .enumerate()
        .map(|(idx, parsed)| {
            let mut chunks = Vec::new();
            extract_function_chunks(
                parsed.tree.root_node(),
                &parsed.source,
                &parsed.path,
                &mut chunks,
            );
            (idx, chunks)
        })
        .collect();
    per_file.sort_by_key(|(idx, _)| *idx);
    let total: usize = per_file.iter().map(|(_, v)| v.len()).sum();
    let mut out = Vec::with_capacity(total);
    for (_, mut v) in per_file {
        out.append(&mut v);
    }
    out
}

#[must_use]
pub fn extract_rust_chunks_for_duplication(parsed_files: &[&ParsedRustFile]) -> Vec<CodeChunk> {
    // Rust AST (`syn::File`) is not Send/Sync, so we keep this sequential.
    // Ordering is naturally stable by input order.
    let mut chunks = Vec::new();
    for parsed in parsed_files {
        extract_rust_function_chunks(&parsed.ast, &parsed.source, &parsed.path, &mut chunks);
    }
    chunks
}

pub(super) fn extract_rust_function_chunks(
    ast: &syn::File,
    source: &str,
    file: &Path,
    chunks: &mut Vec<CodeChunk>,
) {
    for item in &ast.items {
        extract_chunks_from_item(item, source, file, chunks);
    }
}

pub(super) fn extract_chunks_from_item(
    item: &Item,
    source: &str,
    file: &Path,
    chunks: &mut Vec<CodeChunk>,
) {
    match item {
        Item::Fn(func) => {
            let start = func.sig.fn_token.span.start().line;
            let end = func.block.brace_token.span.close().end().line;
            add_rust_function_chunk(
                &func.sig.ident.to_string(),
                start,
                end,
                source,
                file,
                chunks,
            );
        }
        Item::Impl(impl_block) => {
            for impl_item in &impl_block.items {
                if let ImplItem::Fn(method) = impl_item {
                    let start = method.sig.fn_token.span.start().line;
                    let end = method.block.brace_token.span.close().end().line;
                    add_rust_function_chunk(
                        &method.sig.ident.to_string(),
                        start,
                        end,
                        source,
                        file,
                        chunks,
                    );
                }
            }
        }
        Item::Mod(m) => {
            if let Some((_, items)) = &m.content {
                for item in items {
                    extract_chunks_from_item(item, source, file, chunks);
                }
            }
        }
        _ => {}
    }
}

pub(super) fn add_rust_function_chunk(
    name: &str,
    start_line: usize,
    end_line: usize,
    source: &str,
    file: &Path,
    chunks: &mut Vec<CodeChunk>,
) {
    let line_count = end_line.saturating_sub(start_line) + 1;
    let lines: Vec<&str> = source.lines().collect();
    if start_line > 0 && end_line <= lines.len() {
        let body_text: String = lines[start_line - 1..end_line].join("\n");
        let normalized = normalize_code(&body_text);
        if is_nontrivial_chunk(&normalized, line_count) {
            chunks.push(CodeChunk {
                file: file.to_path_buf(),
                name: name.to_string(),
                start_line,
                end_line,
                normalized,
            });
        }
    }
}

pub(super) fn extract_function_chunks(
    node: Node,
    source: &str,
    file: &Path,
    chunks: &mut Vec<CodeChunk>,
) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let name = get_child_by_field(node, "name", source).unwrap_or_default();
            let (start_line, end_line) =
                (node.start_position().row + 1, node.end_position().row + 1);
            let line_count = end_line.saturating_sub(start_line) + 1;
            if let Some(body) = node.child_by_field_name("body") {
                let normalized = normalize_code(&source[body.start_byte()..body.end_byte()]);
                if is_nontrivial_chunk(&normalized, line_count) {
                    chunks.push(CodeChunk {
                        file: file.to_path_buf(),
                        name,
                        start_line,
                        end_line,
                        normalized,
                    });
                }
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_function_chunks(child, source, file, chunks);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_function_chunks(child, source, file, chunks);
            }
        }
    }
}
