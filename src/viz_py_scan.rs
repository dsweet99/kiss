use std::collections::HashSet;

fn trim_leading_dots(s: &str) -> &str {
    s.trim_start_matches('.')
}

fn parse_import_stmt(stmt: &str, out: &mut Vec<String>) {
    let s = stmt.trim_start();
    if let Some(rest) = s.strip_prefix("import ") {
        for part in rest.split(',') {
            let item = part.trim();
            if item.is_empty() {
                continue;
            }
            let name = item.split_whitespace().next().unwrap_or("").trim();
            if !name.is_empty() {
                out.push(name.to_string());
            }
        }
        return;
    }

    if let Some(rest) = s.strip_prefix("from ")
        && let Some((module_part, _)) = rest.split_once(" import ")
    {
        let module = trim_leading_dots(module_part.trim());
        if !module.is_empty() {
            out.push(module.to_string());
        }
    }
}

fn update_bracket_depth(line: &str, depth: &mut i32) {
    for ch in line.chars() {
        match ch {
            '(' | '[' | '{' => *depth += 1,
            ')' | ']' | '}' => *depth -= 1,
            _ => {}
        }
    }
}

fn is_import_start(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("import ") || t.starts_with("from ")
}

/// Fast, line-based Python import extractor for viz.
///
/// We intentionally only look at lines that begin with `import` / `from` (after indentation),
/// and then track basic multiline continuations via brackets and trailing `\`.
#[must_use]
pub fn extract_py_imports_fast(source: &str) -> Vec<String> {
    let mut imports: Vec<String> = Vec::new();
    let mut stmt = String::new();
    let mut depth: i32 = 0;

    for raw_line in source.lines() {
        let line = raw_line.trim_start();
        if stmt.is_empty() && !is_import_start(line) {
            continue;
        }
        // Drop full-line comments early.
        if stmt.is_empty() && line.starts_with('#') {
            continue;
        }

        if !stmt.is_empty() {
            stmt.push(' ');
        }
        stmt.push_str(line);

        update_bracket_depth(line, &mut depth);
        let continued = line.ends_with('\\');

        if depth <= 0 && !continued {
            parse_import_stmt(&stmt, &mut imports);
            stmt.clear();
            depth = 0;
        }
    }
    if !stmt.is_empty() {
        parse_import_stmt(&stmt, &mut imports);
    }

    // Preserve original order but dedup (imports can repeat due to lazy imports).
    let mut seen: HashSet<String> = HashSet::new();
    imports
        .into_iter()
        .filter(|s| seen.insert(s.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_py_imports_fast_basic() {
        let src = r#"
import os, sys
from foo.bar import baz
def f():
    import json
    from ._export_format import X
    x = "import not_a_real_module"
    # import commented_out
"#;
        let got = extract_py_imports_fast(src);
        assert!(got.contains(&"os".to_string()));
        assert!(got.contains(&"sys".to_string()));
        assert!(got.contains(&"foo.bar".to_string()));
        assert!(got.contains(&"json".to_string()));
        assert!(got.contains(&"_export_format".to_string()));
        assert!(!got.contains(&"not_a_real_module".to_string()));
        assert!(!got.contains(&"commented_out".to_string()));
    }

    #[test]
    fn test_extract_py_imports_fast_multiline() {
        let src = r"
from pkg.mod import (
    a,
    b,
)
import one, two, three
";
        let got = extract_py_imports_fast(src);
        assert!(got.contains(&"pkg.mod".to_string()));
        assert!(got.contains(&"one".to_string()));
        assert!(got.contains(&"two".to_string()));
        assert!(got.contains(&"three".to_string()));
    }

    #[test]
    fn test_touch_privates_for_static_coverage() {
        assert_eq!(trim_leading_dots("..x"), "x");

        let mut out = Vec::new();
        parse_import_stmt("import os, sys", &mut out);
        parse_import_stmt("from .foo.bar import Baz", &mut out);
        assert!(out.contains(&"os".to_string()));
        assert!(out.contains(&"sys".to_string()));
        assert!(out.contains(&"foo.bar".to_string()));

        let mut depth = 0;
        update_bracket_depth("(", &mut depth);
        update_bracket_depth(")", &mut depth);
        assert_eq!(depth, 0);

        assert!(is_import_start("  import os"));
        assert!(is_import_start("\tfrom x import y"));
        assert!(!is_import_start("x = 1"));
    }
}

