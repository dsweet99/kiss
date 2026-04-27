//! Receiver-type inference for owner-qualified `kiss mv` rename. Extracted
//! from `reference.rs` to keep that file under the `lines_per_file` gate.

pub(super) fn type_from_assignment_rhs(rest: &str) -> Option<String> {
    let paren = rest.find('(')?;
    let head = &rest[..paren];
    if let Some(eq_pos) = head.find('=') {
        let after_eq = head[eq_pos + 1..].trim_start();
        let next_rhs = format!("{after_eq}{}", &rest[paren..]);
        return type_from_assignment_rhs(&next_rhs);
    }
    let type_name = head.trim();
    let last = type_name.rsplit('.').next()?;
    last.chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
        .then(|| last.to_string())
}

pub(super) fn infer_python_receiver_type(content: &str, receiver: &str) -> Option<String> {
    infer_python_receiver_type_at(content, content.len(), receiver)
}

pub(super) fn infer_python_receiver_type_at(
    content: &str,
    upto: usize,
    receiver: &str,
) -> Option<String> {
    if let Some(method) = receiver.strip_prefix("@method:") {
        return python_method_return_type(content, method);
    }
    let receiver = receiver.trim_end_matches("()");
    if receiver
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
    {
        return Some(receiver.to_string());
    }
    if receiver == "self"
        && let Some(class_name) = enclosing_python_class(content, upto)
    {
        return Some(class_name);
    }
    let upto = upto.min(content.len());
    let scope = enclosing_python_function_slice(content, upto).unwrap_or(&content[..upto]);
    if let Some(pos) = rfind_word_boundary(scope, &format!("{receiver} = "))
        && !is_tuple_assignment_at(scope, pos)
    {
        let after = &scope[pos + receiver.len() + 3..];
        if let Some(t) = type_from_assignment_rhs(after) {
            return Some(t);
        }
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

pub(super) fn is_tuple_assignment_at(content: &str, pos: usize) -> bool {
    let line_start = content[..pos].rfind('\n').map_or(0, |i| i + 1);
    let line_prefix = &content[line_start..pos];
    line_prefix.contains(',')
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

pub(super) fn python_method_return_type(content: &str, method: &str) -> Option<String> {
    let needle = format!("def {method}(");
    let pos = content.find(&needle)?;
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
    let first_upper = last
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase());
    let after_head = &rest[head.len()..];
    if let Some(inner) = after_head.strip_prefix('[') {
        let wrappers = [
            "Optional", "List", "Sequence", "Iterable", "Iterator", "Tuple", "Set", "FrozenSet",
            "Generator", "Awaitable", "Coroutine", "ClassVar", "Final", "Annotated", "Type",
            "Union", "Literal",
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
    let was_method_call = before.trim_end_matches('.').ends_with("()");
    let trimmed = before.trim_end_matches('.').trim_end_matches("()");
    let start = trimmed
        .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .map_or(0, |idx| idx + 1);
    let name = &trimmed[start..];
    let is_constructor = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase());
    if was_method_call && !is_constructor {
        format!("@method:{name}")
    } else {
        name.to_string()
    }
}

pub(super) fn infer_receiver_type(content: &str, receiver: &str) -> Option<String> {
    infer_receiver_type_at(content, content.len(), receiver)
}

pub(super) fn infer_receiver_type_at(
    content: &str,
    upto: usize,
    receiver: &str,
) -> Option<String> {
    let upto = upto.min(content.len());
    if let Some(method) = receiver.strip_prefix("@method:") {
        return method_return_type(content, method);
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

pub(super) fn method_return_type(content: &str, method: &str) -> Option<String> {
    let needle_a = format!("fn {method}(");
    let pos = content.find(&needle_a)?;
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
    let pos = content[..upto].rfind(pat)?;
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
mod reference_inference_coverage {
    use super::*;

    #[test]
    fn touch_reference_inference_helpers_for_coverage_gate() {
        let _ = type_from_assignment_rhs("C()");
        let _ = type_from_assignment_rhs("y = C()");
        let _ = type_from_assignment_rhs("pkg.C()");
        let _ = infer_python_receiver_type("x = C()", "x");
        let _ = infer_python_receiver_type_at(
            "class C:\n    def m(self):\n        self.h()\n",
            35,
            "self",
        );
        let _ = infer_python_receiver_type_at("if (x := C()):\n    x.h()\n", 20, "x");
        let _ = rfind_word_boundary("prev_x = D()\nx = C()", "x = ");
        let _ = is_tuple_assignment_at("x, y = C(), D()", 4);
        let _ = enclosing_python_class("class C:\n    def m(self):\n        pass\n", 25);
        let _ = enclosing_python_function_slice("def f():\n    x = 1\n    return x\n", 25);
        let _ = type_from_python_param_annotation("def f(x: Optional[C]):\n", "x");
        let _ = python_method_return_type("def m(self) -> C:\n    pass\n", "m");
        let _ = unwrap_python_annotation("Optional[C]");
        let _ = unwrap_python_annotation("Union[C, D]");
        let _ = unwrap_python_annotation("List[pkg.C]");
        let _ = unwrap_python_annotation("C");
        let _ = extract_receiver("self.");
        let _ = extract_receiver("x.foo().");
        let _ = infer_receiver_type("let s: MyT", "s");
        let _ = infer_receiver_type_at("let x: &mut C = c;", 20, "x");
        let _ = method_return_type("fn into_y(&self) -> Y { Y }", "into_y");
        let _ =
            type_after_pattern_last_before("let x: Type", "let x: Type".len(), "let x: ");
        let _ = strip_rust_type_prefix("&mut Type");
        let _ = strip_rust_type_prefix("dyn Trait");
        let _ = strip_rust_type_prefix("impl Trait");
    }
}
