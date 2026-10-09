use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::fnmatch::glob_match;

pub(super) fn excluded_by_collect_ignore(
    path: &Path,
    repo_root: &Path,
    cache: &mut HashMap<PathBuf, ConftestIgnore>,
) -> bool {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let path = path.canonicalize().unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        }
    });
    let mut dir = path.parent().map(Path::to_path_buf);
    while let Some(current) = dir {
        if current != root && !current.starts_with(&root) {
            break;
        }
        let rules = cache
            .entry(current.clone())
            .or_insert_with(|| ConftestIgnore::load(&current));
        if rules.matches(&path) {
            return true;
        }
        if current == root {
            break;
        }
        dir = current.parent().map(Path::to_path_buf);
    }
    false
}

pub(super) struct ConftestIgnore {
    dir: PathBuf,
    paths: Vec<String>,
    globs: Vec<String>,
}

impl ConftestIgnore {
    fn load(dir: &Path) -> Self {
        let text = std::fs::read_to_string(dir.join("conftest.py")).unwrap_or_default();
        Self {
            dir: dir.to_path_buf(),
            paths: literal_string_list(&text, "collect_ignore").unwrap_or_default(),
            globs: literal_string_list(&text, "collect_ignore_glob").unwrap_or_default(),
        }
    }

    fn matches(&self, path: &Path) -> bool {
        if self.paths.is_empty() && self.globs.is_empty() {
            return false;
        }
        for entry in &self.paths {
            let ignored = self.dir.join(entry);
            if path.starts_with(&ignored) {
                return true;
            }
        }
        for entry in &self.globs {
            let pattern = self.dir.join(entry);
            let pattern = pattern.to_string_lossy();
            if glob_hits(pattern.as_ref(), path, &self.dir) {
                return true;
            }
        }
        false
    }
}

fn glob_hits(pattern: &str, path: &Path, stop: &Path) -> bool {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if glob_match(pattern, &candidate.to_string_lossy()) {
            return true;
        }
        if candidate == stop {
            break;
        }
        current = candidate.parent();
    }
    false
}

fn literal_string_list(text: &str, name: &str) -> Option<Vec<String>> {
    let mut found = None;
    let mut index = 0;
    while index < text.len() {
        if let Some(next) = python_string_end(text, index) {
            index = next;
            continue;
        }
        if text[index..].starts_with('#') {
            let rest = &text[index..];
            let rel = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
            index += rel;
            continue;
        }
        if assignment_at(text, index, name) {
            let after = text[index + name.len()..].trim_start();
            if let Some(after) = after.strip_prefix('=') {
                let after = after.trim_start();
                if let Some(items) = parse_string_list(after) {
                    found = Some(items);
                }
            }
        }
        index += text[index..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }
    found
}

fn python_string_end(text: &str, index: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let rest = bytes.get(index..)?;
    for prefix_len in [2usize, 1, 0] {
        if rest.len() <= prefix_len {
            continue;
        }
        if prefix_len > 0
            && !rest[..prefix_len]
                .iter()
                .all(|byte| matches!(byte, b'r' | b'R' | b'b' | b'B' | b'f' | b'F' | b'u' | b'U'))
        {
            continue;
        }
        let raw = rest[..prefix_len]
            .iter()
            .any(|byte| *byte == b'r' || *byte == b'R');
        let after = &rest[prefix_len..];
        let delim: &[u8] = if after.starts_with(b"\"\"\"") || after.starts_with(b"'''") {
            &after[..3]
        } else if after.starts_with(b"\"") || after.starts_with(b"'") {
            &after[..1]
        } else {
            continue;
        };
        let mut cursor = index + prefix_len + delim.len();
        while cursor < bytes.len() {
            if !raw && bytes[cursor] == b'\\' {
                cursor += 1;
                if cursor < bytes.len() {
                    cursor += 1;
                }
                continue;
            }
            if bytes[cursor..].starts_with(delim) {
                return Some(cursor + delim.len());
            }
            cursor += 1;
        }
        return Some(bytes.len());
    }
    None
}

fn assignment_at(text: &str, index: usize, name: &str) -> bool {
    let rest = &text[index..];
    if !rest.starts_with(name) {
        return false;
    }
    let after = index + name.len();
    if text[after..]
        .chars()
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        return false;
    }
    if index > 0 {
        let before = text[..index].chars().next_back();
        if before.is_some_and(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
            return false;
        }
    }
    true
}

fn parse_string_list(text: &str) -> Option<Vec<String>> {
    let mut chars = text.chars().peekable();
    if chars.next() != Some('[') {
        return None;
    }
    let mut items = Vec::new();
    loop {
        skip_list_gap(&mut chars);
        match chars.peek().copied() {
            Some(']') => return Some(items),
            Some('"' | '\'') => items.push(parse_quoted(&mut chars)?),
            _ => return None,
        }
        skip_list_gap(&mut chars);
        match chars.next() {
            Some(',') => continue,
            Some(']') => return Some(items),
            _ => return None,
        }
    }
}

fn skip_list_gap(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    loop {
        while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
            chars.next();
        }
        if chars.peek() == Some(&'#') {
            while chars.peek().is_some_and(|ch| *ch != '\n') {
                chars.next();
            }
            continue;
        }
        break;
    }
}

fn parse_quoted(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let quote = chars.next()?;
    let mut value = String::new();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                Some(other) => value.push(other),
                None => return None,
            }
            continue;
        }
        if ch == quote {
            return Some(value);
        }
        value.push(ch);
    }
    None
}
