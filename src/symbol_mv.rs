//! Semantic rename/move (`kiss mv`): query parsing, planning, and transactional apply.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::Language;

// --- CLI / request -----------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MvOptions {
    pub query: String,
    pub new_name: String,
    pub paths: Vec<String>,
    pub to: Option<PathBuf>,
    pub dry_run: bool,
    pub json: bool,
    pub lang_filter: Option<Language>,
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MvRequest {
    pub query: ParsedQuery,
    pub new_name: String,
    pub paths: Vec<String>,
    pub to: Option<PathBuf>,
    pub ignore: Vec<String>,
}

// --- Query -------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ParsedQuery {
    pub raw: String,
    pub path: PathBuf,
    pub symbol: String,
    pub member: Option<String>,
    pub language: Language,
}

impl ParsedQuery {
    pub fn old_name(&self) -> &str {
        self.member.as_deref().unwrap_or(self.symbol.as_str())
    }

    pub const fn language_name(&self) -> &'static str {
        language_name(self.language)
    }
}

pub fn parse_mv_query(raw: &str) -> Result<ParsedQuery, String> {
    let (path_part, symbol_part) = raw
        .split_once("::")
        .ok_or_else(|| "query must contain '::' (e.g. path.py::name)".to_string())?;
    if path_part.is_empty() || symbol_part.is_empty() {
        return Err("query path and symbol must both be non-empty".to_string());
    }
    let path = PathBuf::from(path_part);
    let language = detect_language(&path)?;
    let (symbol, member) = parse_symbol_shape(symbol_part, language)?;
    Ok(ParsedQuery {
        raw: raw.to_string(),
        path,
        symbol,
        member,
        language,
    })
}

pub fn validate_new_name(new_name: &str, language: Language) -> Result<(), String> {
    if new_name.is_empty() {
        return Err("new_name cannot be empty".to_string());
    }
    if new_name.contains('.') || new_name.contains("::") {
        return Err("new_name must be a bare identifier".to_string());
    }
    if !is_valid_identifier(new_name, language) {
        return Err(format!(
            "invalid {} identifier '{}'",
            language_name(language),
            new_name
        ));
    }
    Ok(())
}

fn detect_language(path: &std::path::Path) -> Result<Language, String> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("py") => Ok(Language::Python),
        Some("rs") => Ok(Language::Rust),
        _ => Err("query path must end in .py or .rs".to_string()),
    }
}

fn parse_symbol_shape(
    symbol_part: &str,
    language: Language,
) -> Result<(String, Option<String>), String> {
    if let Some((base, member)) = symbol_part.split_once('.') {
        if member.contains('.') {
            return Err("only one member separator is supported in QUERY".to_string());
        }
        if !is_valid_identifier(base, language) || !is_valid_identifier(member, language) {
            return Err(format!("invalid {} symbol in query", language_name(language)));
        }
        Ok((base.to_string(), Some(member.to_string())))
    } else if !is_valid_identifier(symbol_part, language) {
        Err(format!("invalid {} symbol in query", language_name(language)))
    } else {
        Ok((symbol_part.to_string(), None))
    }
}

fn is_valid_identifier(name: &str, _language: Language) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

pub const fn language_name(language: Language) -> &'static str {
    match language {
        Language::Python => "python",
        Language::Rust => "rust",
    }
}

// --- Plan --------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum EditKind {
    Definition,
    Reference,
}

#[derive(Debug, Clone)]
pub struct PlannedEdit {
    pub path: PathBuf,
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: usize,
    pub old_snippet: String,
    pub new_snippet: String,
    pub kind: EditKind,
}

#[derive(Debug, Clone)]
pub struct MvPlan {
    pub files: Vec<PathBuf>,
    pub edits: Vec<PlannedEdit>,
}

pub fn plan_edits(req: &MvRequest) -> MvPlan {
    let old_name = req.query.old_name();
    let source_path = &req.query.path;
    let source_canonical = source_path.canonicalize().unwrap_or_else(|_| source_path.clone());
    let Ok(source_content) = fs::read_to_string(source_path) else {
        return MvPlan { files: Vec::new(), edits: Vec::new() };
    };

    let mut files = BTreeSet::new();
    let owner = req.query.member.as_ref().map(|_| req.query.symbol.as_str());

    let def_span = find_definition_span(&source_content, old_name, owner, req.query.language);

    let mut edits = collect_source_rename_edits(
        source_path, &source_content, (old_name, &req.new_name),
        owner, req.query.language, def_span, req.to.is_some(),
    );
    files.insert(source_path.clone());

    let candidates = gather_candidate_files(&req.paths, &req.ignore, req.query.language);
    for path in candidates {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if canonical == source_canonical { continue }
        let Ok(content) = fs::read_to_string(&path) else { continue };
        let ref_edits = collect_reference_edits(
            &path, &content, old_name, &req.new_name, owner, req.query.language,
        );
        if !ref_edits.is_empty() {
            files.insert(path);
            edits.extend(ref_edits);
        }
    }

    if let Some((dest_path, remove_edit, insert_edit)) = build_move_edits(
        source_path, &source_content, old_name, &req.new_name, def_span, req.to.as_ref(),
    ) {
        files.insert(dest_path);
        edits.push(remove_edit);
        edits.push(insert_edit);
    }

    edits.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.start_byte.cmp(&b.start_byte)));
    MvPlan { files: files.into_iter().collect(), edits }
}

fn gather_candidate_files(paths: &[String], ignore: &[String], language: Language) -> Vec<PathBuf> {
    let (py_files, rs_files) = crate::discovery::gather_files_by_lang(paths, Some(language), ignore);
    match language {
        Language::Python => py_files,
        Language::Rust => rs_files,
    }
}

fn collect_reference_edits(
    path: &Path,
    content: &str,
    old_name: &str,
    new_name: &str,
    owner: Option<&str>,
    language: Language,
) -> Vec<PlannedEdit> {
    let mut edits = Vec::new();
    for (start, end, line) in find_identifier_occurrences(content, old_name) {
        if is_supported_reference_site(content, start, old_name, owner, language) {
            edits.push(PlannedEdit {
                path: path.to_path_buf(), start_byte: start, end_byte: end, line,
                old_snippet: old_name.to_string(), new_snippet: new_name.to_string(),
                kind: EditKind::Reference,
            });
        }
    }
    edits
}

fn collect_source_rename_edits(
    source_path: &Path,
    source_content: &str,
    names: (&str, &str),
    owner: Option<&str>,
    language: Language,
    def_span: Option<DefinitionSpan>,
    moving: bool,
) -> Vec<PlannedEdit> {
    let (old_name, new_name) = names;
    let mut edits = Vec::new();
    for (start_byte, end_byte, line) in find_identifier_occurrences(source_content, old_name) {
        if !is_supported_reference_site(source_content, start_byte, old_name, owner, language) {
            continue;
        }
        if moving && def_span.is_some_and(|span| span.contains(start_byte)) {
            continue;
        }
        edits.push(PlannedEdit {
            path: source_path.to_path_buf(),
            start_byte,
            end_byte,
            line,
            old_snippet: old_name.to_string(),
            new_snippet: new_name.to_string(),
            kind: if def_span.is_some_and(|span| span.contains(start_byte)) {
                EditKind::Definition
            } else {
                EditKind::Reference
            },
        });
    }
    edits
}

fn build_move_edits(
    source_path: &Path,
    source_content: &str,
    old_name: &str,
    new_name: &str,
    def_span: Option<DefinitionSpan>,
    dest: Option<&PathBuf>,
) -> Option<(PathBuf, PlannedEdit, PlannedEdit)> {
    let span = def_span?;
    let dest_path = dest?.clone();
    let moved = rename_definition_text(&source_content[span.start..span.end], old_name, new_name);
    let existing_dest = fs::read_to_string(&dest_path).unwrap_or_default();
    let needs_newline = !existing_dest.is_empty() && !existing_dest.ends_with('\n');
    let insertion = if needs_newline { format!("\n{moved}") } else { moved };
    Some((
        dest_path.clone(),
        PlannedEdit {
            path: source_path.to_path_buf(),
            start_byte: span.start,
            end_byte: span.end,
            line: line_for_offset(source_content, span.start),
            old_snippet: source_content[span.start..span.end].to_string(),
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

fn rename_definition_text(definition: &str, old_name: &str, new_name: &str) -> String {
    let is_safe = |off: usize| {
        let before = &definition[..off];
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let prefix = &definition[line_start..off];
        if prefix.contains('#') { return false }
        let mut in_str: Option<char> = None;
        for c in prefix.chars() {
            match c {
                '"' | '\'' if in_str == Some(c) => in_str = None,
                '"' | '\'' if in_str.is_none() => in_str = Some(c),
                _ => {}
            }
        }
        in_str.is_none()
    };
    let occs: Vec<_> = find_identifier_occurrences(definition, old_name)
        .into_iter()
        .filter(|(s, _, _)| is_safe(*s))
        .collect();
    if occs.is_empty() { return definition.to_string() }
    let mut out = String::with_capacity(definition.len());
    let mut last = 0;
    for (s, e, _) in occs { out.push_str(&definition[last..s]); out.push_str(new_name); last = e; }
    out.push_str(&definition[last..]);
    out
}

#[derive(Clone, Copy)]
struct DefinitionSpan {
    start: usize,
    end: usize,
}

impl DefinitionSpan {
    const fn contains(self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }
}

fn find_definition_span(
    content: &str,
    method: &str,
    owner: Option<&str>,
    language: Language,
) -> Option<DefinitionSpan> {
    match language {
        Language::Python => find_python_definition_span(content, method, owner),
        Language::Rust => find_rust_definition_span(content, method, owner),
    }
}

fn find_python_definition_span(content: &str, method: &str, owner: Option<&str>) -> Option<DefinitionSpan> {
    let (range_start, range_end) = owner
        .and_then(|o| find_python_class_block(content, o))
        .unwrap_or((0, content.len()));
    let scope = &content[range_start..range_end];
    let needle = format!("def {method}(");
    let mut def_start = None;
    let mut def_indent = 0;
    let mut offset = 0;
    for line in scope.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if def_start.is_none() && trimmed.starts_with(&needle) {
            def_start = Some(range_start + offset);
            def_indent = indent;
        } else if let Some(start) = def_start
            && !trimmed.is_empty() && !trimmed.starts_with('#') && indent <= def_indent
        {
            return Some(DefinitionSpan { start, end: range_start + offset });
        }
        offset += line.len() + 1;
    }
    def_start.map(|start| DefinitionSpan { start, end: range_end })
}

fn find_brace_block_end(content: &str, open_brace: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, ch) in content[open_brace..].char_indices() {
        match ch { '{' => depth += 1, '}' => { depth = depth.saturating_sub(1); if depth == 0 { return Some(open_brace + i + 1) } }, _ => {} }
    }
    None
}

fn find_rust_definition_span(content: &str, method: &str, owner: Option<&str>) -> Option<DefinitionSpan> {
    let (lo, hi) = owner.and_then(|o| find_impl_block(content, o)).unwrap_or((0, content.len()));
    let scope = &content[lo..hi];
    let start = [format!("fn {method}("), format!("pub fn {method}(")]
        .iter().find_map(|c| scope.find(c)).map(|p| lo + p)?;
    let open = start + content[start..].find('{')?;
    find_brace_block_end(content, open).map(|end| DefinitionSpan { start, end })
}

fn find_impl_block(content: &str, owner: &str) -> Option<(usize, usize)> {
    let start = content.find(&format!("impl {owner}"))?;
    let open = start + content[start..].find('{')?;
    find_brace_block_end(content, open).map(|end| (start, end))
}

fn is_supported_reference_site(
    content: &str,
    start: usize,
    ident: &str,
    owner: Option<&str>,
    language: Language,
) -> bool {
    match language {
        Language::Python => is_python_reference_site(content, start, ident, owner),
        Language::Rust => is_rust_reference_site(content, start, ident, owner),
    }
}

fn is_python_reference_site(content: &str, start: usize, ident: &str, owner: Option<&str>) -> bool {
    let before = &content[..start];
    let line_start = before.rfind('\n').map_or(0, |idx| idx + 1);
    let line = &content[line_start..].lines().next().unwrap_or_default();
    let is_def = line.trim_start().starts_with(&format!("def {ident}("));
    if is_def {
        return owner.map_or_else(
            || !is_inside_any_class(content, start),
            |o| find_python_class_block(content, o)
                .is_some_and(|(cls_start, cls_end)| start >= cls_start && start < cls_end),
        );
    }
    let is_import = before.ends_with("import ") || before.ends_with(", ");
    if is_import && owner.is_none() {
        return true;
    }
    let after = &content[(start + ident.len())..];
    let is_method_call = before.ends_with('.');
    match owner {
        Some(_) if !is_method_call => false,
        Some(o) => infer_python_receiver_type(content, &extract_receiver(before)).as_deref() == Some(o),
        None => !is_method_call && after.trim_start().starts_with('('),
    }
}

fn is_inside_any_class(content: &str, offset: usize) -> bool {
    let mut class_indent: Option<usize> = None;
    let mut pos = 0;
    for line in content.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.starts_with("class ") && trimmed.contains(':') { class_indent = Some(indent) }
        else if class_indent.is_some_and(|ci| indent <= ci && !trimmed.is_empty() && !trimmed.starts_with('#')) { class_indent = None }
        if pos <= offset && offset <= pos + line.len() { return class_indent.is_some() }
        pos += line.len() + 1;
    }
    false
}

fn find_python_class_block(content: &str, class_name: &str) -> Option<(usize, usize)> {
    let prefix = format!("class {class_name}");
    let mut offset = 0;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&prefix) && trimmed[prefix.len()..].starts_with([':', '(', ' ']) {
            let base_indent = line.len() - trimmed.len();
            let start = offset;
            let mut end = offset + line.len() + 1;
            for next_line in content[end..].lines() {
                let next_trimmed = next_line.trim_start();
                if !next_trimmed.is_empty() && !next_trimmed.starts_with('#') {
                    let indent = next_line.len() - next_trimmed.len();
                    if indent <= base_indent { break }
                }
                end += next_line.len() + 1;
            }
            return Some((start, end.min(content.len())));
        }
        offset += line.len() + 1;
    }
    None
}

fn infer_python_receiver_type(content: &str, receiver: &str) -> Option<String> {
    let receiver = receiver.trim_end_matches("()");
    if receiver.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return Some(receiver.to_string());
    }
    let pat = format!("{receiver} = ");
    if let Some(pos) = content.find(&pat) {
        let rest = &content[pos + pat.len()..];
        if let Some(paren) = rest.find('(') {
            let type_name = rest[..paren].trim();
            if type_name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                return Some(type_name.to_string());
            }
        }
    }
    None
}

fn is_rust_reference_site(content: &str, start: usize, ident: &str, owner: Option<&str>) -> bool {
    let before = &content[..start];
    let after = &content[(start + ident.len())..];

    let line_start = before.rfind('\n').map_or(0, |idx| idx + 1);
    let line = &content[line_start..].lines().next().unwrap_or_default();

    let is_fn_def = line.contains(&format!("fn {ident}("));
    if is_fn_def {
        return owner.is_none_or(|o| {
            find_impl_block(content, o)
                .is_some_and(|impl_block| start >= impl_block.0 && start < impl_block.1)
        });
    }

    match owner {
        Some(_) if !before.ends_with('.') => false,
        Some(o) => infer_receiver_type(content, &extract_receiver(before)).as_deref() == Some(o),
        None => after.trim_start().starts_with('('),
    }
}

fn extract_receiver(before: &str) -> String {
    let trimmed = before.trim_end_matches('.').trim_end_matches("()");
    let start = trimmed.rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_').map_or(0, |i| i + 1);
    trimmed[start..].to_string()
}

fn infer_receiver_type(content: &str, receiver: &str) -> Option<String> {
    for pat in [format!("let {receiver}: "), format!("let {receiver} : "),
                format!("{receiver}: &"), format!("{receiver}: ")] {
        if let Some(pos) = content.find(&pat) {
            let ty: String = content[pos + pat.len()..].chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !ty.is_empty() { return Some(ty) }
        }
    }
    None
}

fn find_identifier_occurrences(content: &str, ident: &str) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(found) = content[search_from..].find(ident) {
        let start = search_from + found;
        let end = start + ident.len();
        let left_ok = start == 0 || !is_ident_char(content.as_bytes()[start - 1] as char);
        let right_ok = end == content.len() || !is_ident_char(content.as_bytes()[end] as char);
        if left_ok && right_ok {
            out.push((start, end, line_for_offset(content, start)));
        }
        search_from = end;
    }
    out
}

const fn is_ident_char(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn line_for_offset(content: &str, offset: usize) -> usize {
    content[..offset].chars().filter(|&c| c == '\n').count() + 1
}

// --- Apply -------------------------------------------------------------------

pub fn apply_plan_transactional(plan: &MvPlan) -> Result<(), String> {
    check_for_overlaps(plan)?;

    let mut originals: BTreeMap<PathBuf, String> = BTreeMap::new();
    for path in &plan.files {
        let existing = fs::read_to_string(path).unwrap_or_default();
        originals.insert(path.clone(), existing);
    }

    let mut per_file_edits: BTreeMap<PathBuf, Vec<&PlannedEdit>> = BTreeMap::new();
    for edit in &plan.edits {
        per_file_edits.entry(edit.path.clone()).or_default().push(edit);
    }

    for (path, edits) in &mut per_file_edits {
        let Some(source) = originals.get(path) else {
            return Err(format!("missing source snapshot for {}", path.display()));
        };
        let mut updated = source.clone();
        edits.sort_by(|a, b| b.start_byte.cmp(&a.start_byte));
        for edit in edits.iter() {
            if edit.end_byte > updated.len() || edit.start_byte > edit.end_byte {
                rollback(&originals)?;
                return Err(format!(
                    "invalid edit range {}..{} for {}",
                    edit.start_byte,
                    edit.end_byte,
                    path.display()
                ));
            }
            updated.replace_range(edit.start_byte..edit.end_byte, &edit.new_snippet);
        }
        if let Err(e) = fs::write(path, updated) {
            rollback(&originals)?;
            return Err(format!("failed writing {}: {e}", path.display()));
        }
    }
    Ok(())
}

fn check_for_overlaps(plan: &MvPlan) -> Result<(), String> {
    let mut by_file: BTreeMap<&PathBuf, Vec<(usize, usize)>> = BTreeMap::new();
    for edit in &plan.edits {
        by_file
            .entry(&edit.path)
            .or_default()
            .push((edit.start_byte, edit.end_byte));
    }
    for (path, mut ranges) in by_file {
        ranges.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        for pair in ranges.windows(2) {
            if pair[0].1 > pair[1].0 {
                return Err(format!(
                    "overlapping edits in {}: {}..{} overlaps {}..{}",
                    path.display(),
                    pair[0].0,
                    pair[0].1,
                    pair[1].0,
                    pair[1].1
                ));
            }
        }
    }
    Ok(())
}

fn rollback(originals: &BTreeMap<PathBuf, String>) -> Result<(), String> {
    for (path, content) in originals {
        fs::write(path, content).map_err(|e| format!("rollback failed for {}: {e}", path.display()))?;
    }
    Ok(())
}

// --- Orchestration -----------------------------------------------------------

pub fn run_mv_command(opts: MvOptions) -> i32 {
    i32::from(run_mv_inner(opts).is_err())
}

fn validate_mv_options(opts: &MvOptions) -> Result<ParsedQuery, ()> {
    let query = parse_mv_query(&opts.query).map_err(|e| eprintln!("Error: {e}"))?;
    if let Some(lang_filter) = opts.lang_filter
        && lang_filter != query.language
    {
        eprintln!(
            "Error: query language ({}) does not match --lang ({})",
            query.language_name(),
            language_name(lang_filter)
        );
        return Err(());
    }
    validate_new_name(&opts.new_name, query.language).map_err(|e| eprintln!("Error: {e}"))?;
    if opts.to.is_some() && query.member.is_some() {
        eprintln!(
            "Error: --to moves are only supported for top-level functions, not methods (got {})",
            query.raw
        );
        return Err(());
    }
    Ok(query)
}

fn run_mv_inner(opts: MvOptions) -> Result<(), ()> {
    let query = validate_mv_options(&opts)?;
    let req = MvRequest {
        query,
        new_name: opts.new_name,
        paths: opts.paths,
        to: opts.to,
        ignore: opts.ignore,
    };

    let plan = plan_edits(&req);
    if plan.edits.is_empty() {
        eprintln!("Error: no symbol occurrences found for '{}'", req.query.raw);
        return Err(());
    }

    if opts.json {
        print_json_plan(&plan).map_err(|e| eprintln!("Error: failed to serialize plan: {e}"))?;
    } else {
        print_human_plan(&plan);
        if !opts.dry_run {
            apply_plan_transactional(&plan).map_err(|e| eprintln!("Error: {e}"))?;
        }
    }
    Ok(())
}

fn print_human_plan(plan: &MvPlan) {
    for edit in &plan.edits {
        println!(
            "{}:{}: {} -> {}",
            edit.path.display(),
            edit.line,
            edit.old_snippet,
            edit.new_snippet
        );
    }
}

fn print_json_plan(plan: &MvPlan) -> Result<(), serde_json::Error> {
    let edits: Vec<serde_json::Value> = plan
        .edits
        .iter()
        .map(|edit| {
            serde_json::json!({
                "start_byte": edit.start_byte,
                "end_byte": edit.end_byte,
                "line": edit.line,
                "kind": edit_kind_name(edit.kind),
                "old_snippet": edit.old_snippet,
                "new_snippet": edit.new_snippet,
                "path": edit.path.display().to_string(),
            })
        })
        .collect();
    let payload = serde_json::json!({
        "files": plan.files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "edits": edits,
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

const fn edit_kind_name(kind: EditKind) -> &'static str {
    match kind {
        EditKind::Definition => "definition",
        EditKind::Reference => "reference",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_python_function_query() {
        let q = parse_mv_query("a/b.py::foo").unwrap();
        assert_eq!(q.path, PathBuf::from("a/b.py"));
        assert_eq!(q.symbol, "foo");
        assert_eq!(q.member, None);
        assert_eq!(q.language, Language::Python);
    }

    #[test]
    fn parse_rust_method_query() {
        let q = parse_mv_query("src/lib.rs::Type.method").unwrap();
        assert_eq!(q.symbol, "Type");
        assert_eq!(q.member, Some("method".to_string()));
        assert_eq!(q.old_name(), "method");
    }

    #[test]
    fn reject_bad_queries() {
        assert!(parse_mv_query("missing_separator").is_err());
        assert!(parse_mv_query("a.txt::foo").is_err());
        assert!(parse_mv_query("a.py::foo.bar.baz").is_err());
    }

    #[test]
    fn validate_new_name_rules() {
        assert!(validate_new_name("new_name", Language::Python).is_ok());
        assert!(validate_new_name("new::name", Language::Rust).is_err());
        assert!(validate_new_name("", Language::Rust).is_err());
        assert!(validate_new_name("1bad", Language::Python).is_err());
    }

    #[test]
    fn identifier_boundaries_work() {
        let hits = find_identifier_occurrences("foo food _foo foo()", "foo");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn mv_json_mode_is_valid_json() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("a.py");
        fs::write(&source, "def foo():\n    return 1\nfoo()\n").unwrap();

        let opts = MvOptions {
            query: format!("{}::foo", source.display()),
            new_name: "bar".to_string(),
            paths: vec![tmp.path().display().to_string()],
            to: None,
            dry_run: true,
            json: true,
            lang_filter: Some(Language::Python),
            ignore: vec![],
        };

        assert_eq!(run_mv_command(opts), 0);
    }

    #[test]
    fn mv_rejects_mismatched_lang_filter() {
        let opts = MvOptions {
            query: "src/foo.py::bar".to_string(),
            new_name: "baz".to_string(),
            paths: vec![".".to_string()],
            to: None,
            dry_run: true,
            json: false,
            lang_filter: Some(Language::Rust),
            ignore: vec![],
        };
        assert_eq!(run_mv_command(opts), 1);
    }

    #[test]
    fn applies_rename() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("a.py");
        fs::write(&file, "foo()\n").unwrap();
        let good_plan = MvPlan {
            files: vec![file.clone()],
            edits: vec![PlannedEdit {
                path: file.clone(),
                start_byte: 0,
                end_byte: 3,
                line: 1,
                old_snippet: "foo".to_string(),
                new_snippet: "bar".to_string(),
                kind: EditKind::Reference,
            }],
        };
        apply_plan_transactional(&good_plan).unwrap();
        let updated = fs::read_to_string(&file).unwrap();
        assert_eq!(updated, "bar()\n");
    }

    #[test]
    fn overlap_fails() {
        let file = PathBuf::from("fake.py");
        let plan = MvPlan {
            files: vec![file.clone()],
            edits: vec![
                PlannedEdit {
                    path: file.clone(),
                    start_byte: 0,
                    end_byte: 3,
                    line: 1,
                    old_snippet: "foo".to_string(),
                    new_snippet: "bar".to_string(),
                    kind: EditKind::Reference,
                },
                PlannedEdit {
                    path: file,
                    start_byte: 2,
                    end_byte: 5,
                    line: 1,
                    old_snippet: "o()".to_string(),
                    new_snippet: "xx".to_string(),
                    kind: EditKind::Reference,
                },
            ],
        };
        assert!(apply_plan_transactional(&plan).is_err());
    }

    #[test]
    fn regression_rust_method_query_should_not_rename_other_types() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("mod.rs");
        fs::write(
            &file,
            "struct A;\nstruct B;\n\nimpl A {\n    fn foo(&self) {}\n}\n\nimpl B {\n    fn foo(&self) {}\n}\n\nfn call(a: &A, b: &B) {\n    a.foo();\n    b.foo();\n}\n",
        )
        .unwrap();

        let opts = MvOptions {
            query: format!("{}::A.foo", file.display()),
            new_name: "bar".to_string(),
            paths: vec![tmp.path().display().to_string()],
            to: None,
            dry_run: false,
            json: false,
            lang_filter: Some(Language::Rust),
            ignore: vec![],
        };

        assert_eq!(run_mv_command(opts), 0);
        let updated = fs::read_to_string(&file).unwrap();
        assert!(updated.contains("impl A {\n    fn bar(&self) {}"));
        assert!(updated.contains("a.bar();"));
        assert!(
            updated.contains("impl B {\n    fn foo(&self) {}"),
            "unrelated type method should remain unchanged"
        );
        assert!(
            updated.contains("b.foo();"),
            "unrelated method call should remain unchanged"
        );
    }

    #[test]
    fn regression_move_to_destination_should_relocate_definition() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("source.py");
        let dest = tmp.path().join("dest.py");
        fs::write(&src, "def foo():\n    return 1\n\nvalue = foo()\n").unwrap();
        fs::write(&dest, "def other():\n    return 2\n").unwrap();

        let opts = MvOptions {
            query: format!("{}::foo", src.display()),
            new_name: "foo".to_string(),
            paths: vec![tmp.path().display().to_string()],
            to: Some(dest.clone()),
            dry_run: false,
            json: false,
            lang_filter: Some(Language::Python),
            ignore: vec![],
        };

        assert_eq!(run_mv_command(opts), 0);
        let updated_src = fs::read_to_string(&src).unwrap();
        let updated_dest = fs::read_to_string(&dest).unwrap();
        assert!(
            !updated_src.contains("def foo("),
            "source definition should be removed after move"
        );
        assert!(
            updated_dest.contains("def foo("),
            "destination should contain moved definition"
        );
    }

    #[test]
    fn regression_python_method_should_scope_to_class() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("mod.py");
        fs::write(
            &file,
"class A:\n    def foo(self):\n        pass\n\nclass B:\n    def foo(self):\n        pass\n\ndef use_them():\n    A().foo()\n    B().foo()\n",
        )
        .unwrap();

        let opts = MvOptions {
            query: format!("{}::A.foo", file.display()),
            new_name: "bar".to_string(),
            paths: vec![tmp.path().display().to_string()],
            to: None,
            dry_run: false,
            json: false,
            lang_filter: Some(Language::Python),
            ignore: vec![],
        };

        assert_eq!(run_mv_command(opts), 0);
        let updated = fs::read_to_string(&file).unwrap();
        assert!(
            updated.contains("def bar(self):"),
            "A.foo should be renamed to bar"
        );
        assert!(
            updated.contains("class B:\n    def foo(self):"),
            "B.foo should remain unchanged"
        );
    }
}
