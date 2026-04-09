//! Semantic rename/move (`kiss mv`): query parsing, planning, and transactional apply.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

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
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let first_ok = first == '_' || first.is_ascii_alphabetic();
    first_ok && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
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
    let mut files = BTreeSet::new();
    let mut edits = Vec::new();
    let old_name = req.query.old_name();
    let candidates = match req.query.language {
        Language::Python => python_candidates(&req.paths, &req.ignore),
        Language::Rust => rust_candidates(&req.paths, &req.ignore),
    };

    for path in candidates {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        for (start_byte, end_byte, line) in find_identifier_occurrences(&content, old_name) {
            files.insert(path.clone());
            let kind = if path == req.query.path {
                EditKind::Definition
            } else {
                EditKind::Reference
            };
            edits.push(PlannedEdit {
                path: path.clone(),
                start_byte,
                end_byte,
                line,
                old_snippet: old_name.to_string(),
                new_snippet: req.new_name.clone(),
                kind,
            });
        }
    }

    edits.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.start_byte.cmp(&b.start_byte))
    });

    if let Some(dest) = &req.to {
        files.insert(dest.clone());
    }

    MvPlan {
        files: files.into_iter().collect::<Vec<_>>(),
        edits,
    }
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
    content
        .char_indices()
        .take_while(|(idx, _)| *idx < offset)
        .filter(|(_, c)| *c == '\n')
        .count()
        + 1
}

fn python_candidates(paths: &[String], ignore: &[String]) -> Vec<PathBuf> {
    let (files, _) = crate::discovery::gather_files_by_lang(paths, Some(Language::Python), ignore);
    files
}

fn rust_candidates(paths: &[String], ignore: &[String]) -> Vec<PathBuf> {
    let (_, files) = crate::discovery::gather_files_by_lang(paths, Some(Language::Rust), ignore);
    files
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

fn run_mv_inner(opts: MvOptions) -> Result<(), ()> {
    let query = parse_mv_query(&opts.query).map_err(|e| {
        eprintln!("Error: {e}");
    })?;
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
    validate_new_name(&opts.new_name, query.language).map_err(|e| {
        eprintln!("Error: {e}");
    })?;

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
        print_json_plan(&plan).map_err(|e| {
            eprintln!("Error: failed to serialize plan: {e}");
        })?;
        return Ok(());
    }
    print_human_plan(&plan);
    if opts.dry_run {
        return Ok(());
    }

    apply_plan_transactional(&plan).map_err(|e| {
        eprintln!("Error: {e}");
    })
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
}
