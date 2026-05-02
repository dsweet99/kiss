pub(crate) fn type_from_assignment_rhs(rest: &str) -> Option<String> {
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

#[allow(dead_code)]
pub(crate) fn is_tuple_assignment_at(content: &str, pos: usize) -> bool {
    let line_start = content[..pos].rfind('\n').map_or(0, |i| i + 1);
    let line_prefix = &content[line_start..pos];
    line_prefix.contains(',')
}

pub(crate) fn type_from_assignment_target(
    scope: &str,
    pos: usize,
    receiver: &str,
) -> Option<String> {
    let line_start = scope[..pos].rfind('\n').map_or(0, |i| i + 1);
    let line_prefix = &scope[line_start..pos];
    if line_prefix.contains(',') {
        return tuple_assignment_receiver_type(scope, receiver);
    }
    type_from_assignment_rhs(&scope[pos + receiver.len() + 3..])
}

pub(crate) fn tuple_assignment_receiver_type(scope: &str, receiver: &str) -> Option<String> {
    for line in scope.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || !line.contains('=')
            || !line.contains(',')
        {
            continue;
        }
        if !line.contains(receiver) {
            continue;
        }
        let eq_pos = line.rfind('=')?;
        let lhs = line[..eq_pos].trim_end();
        let rhs = line[eq_pos + 1..].trim_start();
        let targets: Vec<_> = lhs
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect();
        let target_idx = targets.iter().position(|t| *t == receiver)?;
        let rhs_parts = split_top_level_commas(rhs);
        let rhs_expr = rhs_parts.get(target_idx)?.trim();
        return type_from_assignment_rhs(rhs_expr);
    }
    None
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth_paren = 0_usize;
    let mut depth_bracket = 0_usize;
    let mut depth_brace = 0_usize;
    let mut start = 0_usize;
    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => depth_paren += 1,
            ')' => depth_paren = depth_paren.saturating_sub(1),
            '[' => depth_bracket += 1,
            ']' => depth_bracket = depth_bracket.saturating_sub(1),
            '{' => depth_brace += 1,
            '}' => depth_brace = depth_brace.saturating_sub(1),
            ',' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                parts.push(&s[start..idx]);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_assignment_reference_helpers() {
        assert_eq!(
            type_from_assignment_rhs("Helper()"),
            Some("Helper".to_string())
        );
        assert_eq!(type_from_assignment_rhs("C()"), Some("C".to_string()));
        assert!(is_tuple_assignment_at("x, y = a, b", 4));
        assert_eq!(type_from_assignment_target("x, y = Foo, Bar", 0, "x"), None);
        assert_eq!(
            tuple_assignment_receiver_type("x, y = Foo(bar), Baz", "x"),
            Some("Foo".to_string())
        );
        assert_eq!(split_top_level_commas("Foo, Bar, Baz").len(), 3);
        assert_eq!(
            split_top_level_commas("Foo((a,b),c), Bar"),
            vec!["Foo((a,b),c)", " Bar"]
        );
    }
}
