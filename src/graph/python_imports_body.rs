pub(crate) fn push_dotted_segments(raw: &str, modules: &mut Vec<String>) {
    // Treat dotted module paths as a single import target (e.g., "foo.bar", not ["foo", "bar"]).
    // Splitting creates spurious edges to unrelated local modules named like common segments
    // ("utils", "types", "errors", etc.), which can inflate SCC cycles dramatically.
    let trimmed = raw.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        return;
    }
    modules.push(trimmed.to_string());
}

pub(crate) fn read_base_module(child: Node, source: &str, modules: &mut Vec<String>) -> Option<String> {
    let m = child.child_by_field_name("module_name")?;
    let full_module = &source[m.start_byte()..m.end_byte()];
    let trimmed = full_module.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        return None;
    }
    let s = trimmed.to_string();
    modules.push(s.clone());
    Some(s)
}

pub(crate) fn collect_imported_name_candidates(
    child: Node,
    source: &str,
    imported_names: &mut Vec<String>,
) {
    let mut seen_import = false;
    let mut cursor = child.walk();
    for c in child.children(&mut cursor) {
        match c.kind() {
            "import" => seen_import = true,
            "dotted_name" if seen_import => {
                push_import_name_segments(c, source, imported_names);
            }
            "aliased_import" if seen_import => {
                if let Some(n) = c.child_by_field_name("name") {
                    push_import_name_segments(n, source, imported_names);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn extract_modules_from_import_from(child: Node, source: &str) -> Vec<String> {
    let mut modules = Vec::new();
    let base_module = read_base_module(child, source, &mut modules);

    let mut imported_names = Vec::new();
    collect_imported_name_candidates(child, source, &mut imported_names);

    if let Some(base) = base_module {
        for name in imported_names {
            modules.push(format!("{base}.{name}"));
        }
    } else if modules.is_empty() {
        modules.extend(imported_names);
    }
    modules
}

pub(crate) fn push_import_name_segments(node: Node, source: &str, imports: &mut Vec<String>) {
    let name = &source[node.start_byte()..node.end_byte()];
    push_dotted_segments(name, imports);
}

fn collect_import_names(node: Node, source: &str, imports: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => push_import_name_segments(child, source, imports),
            "aliased_import" => {
                if let Some(n) = child.child_by_field_name("name") {
                    push_import_name_segments(n, source, imports);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn extract_imports_for_cache(node: Node, source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    extract_imports_recursive(node, source, &mut imports);
    imports
}

pub(crate) fn extract_imports_recursive(node: Node, source: &str, imports: &mut Vec<String>) {
    match node.kind() {
        "import_statement" => collect_import_names(node, source, imports),
        "import_from_statement" => imports.extend(extract_modules_from_import_from(node, source)),
        "call" => {
            if let Some(m) = extract_dynamic_import_module(node, source) {
                push_dotted_segments(&m, imports);
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_imports_recursive(child, source, imports);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_imports_recursive(child, source, imports);
            }
        }
    }
}

pub(crate) fn is_importlib_import_module(func: Node, source: &str) -> bool {
    if func.kind() != "attribute" {
        return false;
    }
    let Some(obj) = func.child_by_field_name("object") else {
        return false;
    };
    let Some(attr) = func.child_by_field_name("attribute") else {
        return false;
    };
    let Ok(obj_txt) = obj.utf8_text(source.as_bytes()) else {
        return false;
    };
    let Ok(attr_txt) = attr.utf8_text(source.as_bytes()) else {
        return false;
    };
    obj.kind() == "identifier" && obj_txt == "importlib" && attr_txt == "import_module"
}

pub(crate) fn is_dunder_import(func: Node, source: &str) -> bool {
    func.kind() == "identifier"
        && func
            .utf8_text(source.as_bytes())
            .is_ok_and(|s| s == "__import__")
}

pub(crate) fn extract_dynamic_import_module(call: Node, source: &str) -> Option<String> {
    let func = call.child_by_field_name("function")?;
    if !is_importlib_import_module(func, source) && !is_dunder_import(func, source) {
        return None;
    }

    let args = call.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.kind() == "string" {
            return parse_python_string_literal(child, source);
        }
    }
    None
}

pub(crate) fn strip_rbub_prefix(raw: &str) -> Option<(usize, &str)> {
    let mut i = 0;
    for ch in raw.chars() {
        match ch {
            'r' | 'R' | 'u' | 'U' | 'b' | 'B' => i += ch.len_utf8(),
            'f' | 'F' => return None,
            _ => break,
        }
    }
    Some((i, raw.get(i..)?.trim()))
}

pub(crate) fn unquote_triple(s: &str, quote: char) -> Option<String> {
    let q3 = format!("{quote}{quote}{quote}");
    let triple = s.starts_with(&q3);
    if !triple {
        return None;
    }
    (s.ends_with(&q3) && s.len() >= q3.len() * 2)
        .then(|| s[q3.len()..(s.len() - q3.len())].to_string())
}

pub(crate) fn unquote_single(s: &str, quote: char) -> Option<String> {
    (s.len() >= 2 && s.ends_with(quote)).then(|| s[1..(s.len() - 1)].to_string())
}

pub(crate) fn parse_python_string_literal(node: Node, source: &str) -> Option<String> {
    let raw = node.utf8_text(source.as_bytes()).ok()?.trim();
    if raw.is_empty() {
        return None;
    }
    let (_, s) = strip_rbub_prefix(raw)?;
    let quote = s.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let q3 = format!("{quote}{quote}{quote}");
    if s.starts_with(&q3) {
        return unquote_triple(s, quote);
    }
    unquote_single(s, quote)
}
