//! Shared function-signature detection helpers for `kiss mv`.
//!
//! Centralizing declaration-pattern checks here keeps Python and Rust
//! detection behavior consistent between definition discovery and reference
//! classification.

const fn is_ident_char(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

fn has_identifier_boundary(s: &str, ident: &str) -> bool {
    let Some(rest) = s.strip_prefix(ident) else {
        return false;
    };
    rest.chars().next().is_none_or(|c| !is_ident_char(c))
}

pub(super) fn is_python_def_line(line: &str, ident: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(&format!("def {ident}("))
        || trimmed.starts_with(&format!("async def {ident}("))
}

pub(super) fn is_rust_fn_definition_line(line: &str, ident: &str) -> bool {
    let mut rest = line.trim_start();
    loop {
        if rest.starts_with("pub(") {
            if let Some(close) = rest.find(')') {
                rest = rest[close + 1..].trim_start();
                continue;
            }
            return false;
        }
        if let Some(stripped) = rest.strip_prefix("pub ") {
            rest = stripped.trim_start();
            continue;
        }
        if let Some(stripped) = rest.strip_prefix("const ") {
            rest = stripped.trim_start();
            continue;
        }
        if let Some(stripped) = rest.strip_prefix("async ") {
            rest = stripped.trim_start();
            continue;
        }
        if let Some(stripped) = rest.strip_prefix("unsafe ") {
            rest = stripped.trim_start();
            continue;
        }
        if let Some(stripped) = rest.strip_prefix("extern ") {
            rest = stripped.trim_start();
            continue;
        }
        break;
    }

    let Some(rest) = rest.strip_prefix("fn ") else {
        return false;
    };
    has_identifier_boundary(rest, ident)
}

#[cfg(test)]
mod signature_coverage {
    use super::*;

    #[test]
    fn python_def_lines_include_async() {
        assert!(is_python_def_line("async def helper():", "helper"));
        assert!(!is_python_def_line("defhelper():", "helper"));
    }

    #[test]
    fn rust_fn_lines_include_visibility_and_async() {
        assert!(is_rust_fn_definition_line("fn helper() {}", "helper"));
        assert!(is_rust_fn_definition_line("pub fn helper() {}", "helper"));
        assert!(is_rust_fn_definition_line(
            "pub(crate) fn helper() {}",
            "helper"
        ));
        assert!(is_rust_fn_definition_line("async fn helper() {}", "helper"));
        assert!(is_rust_fn_definition_line(
            "pub async fn helper() {}",
            "helper"
        ));
        assert!(is_rust_fn_definition_line(
            "pub(crate) async fn helper() {}",
            "helper"
        ));
        assert!(!is_rust_fn_definition_line("let helper() =", "helper"));
    }

    #[test]
    fn private_signature_helpers_have_expected_behavior() {
        assert!(is_ident_char('a'));
        assert!(!is_ident_char('$'));
        assert!(has_identifier_boundary("helper(", "helper"));
        assert!(!has_identifier_boundary("helperx(", "helper"));
    }
}
