use std::path::Path;

pub(super) fn fnmatch_ex(pattern: &str, path: &Path) -> bool {
    let sep = std::path::MAIN_SEPARATOR;
    if !pattern.contains(sep) {
        return path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| glob_match(pattern, name));
    }
    let path_text = path.to_string_lossy();
    let pattern_is_abs = pattern.starts_with(sep);
    if path.is_absolute() && !pattern_is_abs {
        return glob_match(&format!("*{sep}{pattern}"), path_text.as_ref());
    }
    glob_match(pattern, path_text.as_ref())
}

pub(super) fn glob_match(pattern: &str, name: &str) -> bool {
    glob_match_from(pattern.as_bytes(), name.as_bytes())
}

fn glob_match_from(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.first() {
        None => name.is_empty(),
        Some(b'*') => {
            glob_match_from(&pattern[1..], name)
                || (!name.is_empty() && glob_match_from(pattern, &name[1..]))
        }
        Some(b'?') => !name.is_empty() && glob_match_from(&pattern[1..], &name[1..]),
        Some(b'[') => match character_class(&pattern[1..]) {
            Some((class, rest)) => {
                !name.is_empty()
                    && class_matches(class, name[0])
                    && glob_match_from(rest, &name[1..])
            }
            None => name.first() == Some(&b'[') && glob_match_from(&pattern[1..], &name[1..]),
        },
        Some(&byte) => name.first() == Some(&byte) && glob_match_from(&pattern[1..], &name[1..]),
    }
}

fn character_class(pattern: &[u8]) -> Option<(&[u8], &[u8])> {
    if pattern.is_empty() {
        return None;
    }
    let mut end = 0;
    if pattern[0] == b'!' {
        end = 1;
    }
    if end < pattern.len() && pattern[end] == b']' {
        end += 1;
    }
    while end < pattern.len() && pattern[end] != b']' {
        end += 1;
    }
    if end >= pattern.len() {
        return None;
    }
    Some((&pattern[..end], &pattern[end + 1..]))
}

fn class_matches(class: &[u8], ch: u8) -> bool {
    let (negated, body) = if class.first() == Some(&b'!') {
        (true, &class[1..])
    } else {
        (false, class)
    };
    let mut index = 0;
    let mut matched = false;
    while index < body.len() {
        let is_range = index + 2 < body.len()
            && body[index + 1] == b'-'
            && body[index] != b'-'
            && body[index + 2] != b'-';
        if is_range {
            let start = body[index];
            let end = body[index + 2];
            let (lo, hi) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            if ch >= lo && ch <= hi {
                matched = true;
                break;
            }
            index += 3;
            continue;
        }
        if body[index] == ch {
            matched = true;
            break;
        }
        index += 1;
    }
    if negated { !matched } else { matched }
}
