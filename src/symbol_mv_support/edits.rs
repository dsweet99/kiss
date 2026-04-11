use std::fs;
use std::path::{Path, PathBuf};

use crate::symbol_mv::{EditKind, PlannedEdit};
use crate::Language;

use super::basics::detect_language;
use super::definition::DefinitionSpan;
use super::identifiers::find_identifier_occurrences;
use super::identifiers::line_for_offset;
use super::lex::is_code_offset;
use super::reference::{is_supported_reference_site, RefSiteCtx};

pub struct ReferenceRenameParams<'a> {
    pub path: &'a Path,
    pub content: &'a str,
    pub old_name: &'a str,
    pub new_name: &'a str,
    pub owner: Option<&'a str>,
    pub language: Language,
}

pub fn collect_reference_edits(p: &ReferenceRenameParams<'_>) -> Vec<PlannedEdit> {
    find_identifier_occurrences(p.content, p.old_name)
        .into_iter()
        .filter(|(start, _, _)| {
            is_supported_reference_site(
                &RefSiteCtx {
                    content: p.content,
                    start: *start,
                    ident: p.old_name,
                    owner: p.owner,
                },
                p.language,
            )
        })
        .map(|(start_byte, end_byte, line)| PlannedEdit {
            path: p.path.to_path_buf(),
            start_byte,
            end_byte,
            line,
            old_snippet: p.old_name.to_string(),
            new_snippet: p.new_name.to_string(),
            kind: EditKind::Reference,
        })
        .collect()
}

pub struct SourceRenameParams<'a> {
    pub source_path: &'a Path,
    pub source_content: &'a str,
    pub old_name: &'a str,
    pub new_name: &'a str,
    pub owner: Option<&'a str>,
    pub language: Language,
    pub def_span: Option<DefinitionSpan>,
    pub moving: bool,
}

pub fn collect_source_rename_edits(p: &SourceRenameParams<'_>) -> Vec<PlannedEdit> {
    find_identifier_occurrences(p.source_content, p.old_name)
        .into_iter()
        .filter(|(start, _, _)| {
            is_supported_reference_site(
                &RefSiteCtx {
                    content: p.source_content,
                    start: *start,
                    ident: p.old_name,
                    owner: p.owner,
                },
                p.language,
            ) && !(p.moving && p.def_span.is_some_and(|span| span.contains(*start)))
        })
        .map(|(start_byte, end_byte, line)| PlannedEdit {
            path: p.source_path.to_path_buf(),
            start_byte,
            end_byte,
            line,
            old_snippet: p.old_name.to_string(),
            new_snippet: p.new_name.to_string(),
            kind: if p.def_span.is_some_and(|span| span.contains(start_byte)) {
                EditKind::Definition
            } else {
                EditKind::Reference
            },
        })
        .collect()
}

pub struct MoveEditsParams<'a> {
    pub source_path: &'a Path,
    pub source_content: &'a str,
    pub old_name: &'a str,
    pub new_name: &'a str,
    pub def_span: Option<DefinitionSpan>,
    pub dest: Option<&'a PathBuf>,
}

pub fn build_move_edits(p: &MoveEditsParams<'_>) -> Option<(PathBuf, PlannedEdit, PlannedEdit)> {
    let span = p.def_span?;
    let dest_path = p.dest?.clone();
    let moved = rename_definition_text(
        &p.source_content[span.start..span.end],
        p.old_name,
        p.new_name,
        detect_language(p.source_path).ok()?,
    );
    let existing_dest = fs::read_to_string(&dest_path).unwrap_or_default();
    let insertion = if existing_dest.is_empty() || existing_dest.ends_with('\n') {
        moved
    } else {
        format!("\n{moved}")
    };
    Some((
        dest_path.clone(),
        PlannedEdit {
            path: p.source_path.to_path_buf(),
            start_byte: span.start,
            end_byte: span.end,
            line: line_for_offset(p.source_content, span.start),
            old_snippet: p.source_content[span.start..span.end].to_string(),
            new_snippet: String::new(),
            kind: EditKind::Definition,
        },
        PlannedEdit {
            path: dest_path,
            start_byte: existing_dest.len(),
            end_byte: existing_dest.len(),
            line: existing_dest.lines().count().max(1),
            old_snippet: String::new(),
            new_snippet: insertion,
            kind: EditKind::Definition,
        },
    ))
}

fn rename_definition_text(
    definition: &str,
    old_name: &str,
    new_name: &str,
    language: Language,
) -> String {
    let occs: Vec<_> = find_identifier_occurrences(definition, old_name)
        .into_iter()
        .filter(|(start, _, _)| is_code_offset(definition, *start, language))
        .collect();
    if occs.is_empty() {
        return definition.to_string();
    }
    let mut out = String::with_capacity(definition.len());
    let mut last = 0;
    for (start, end, _) in occs {
        out.push_str(&definition[last..start]);
        out.push_str(new_name);
        last = end;
    }
    out.push_str(&definition[last..]);
    out
}

#[cfg(test)]
mod edits_coverage {
    use super::*;

    #[test]
    fn rename_definition_text_replaces_name() {
        let out = rename_definition_text("def foo(self): pass", "foo", "bar", Language::Python);
        assert!(out.contains("def bar("));
    }
}
