pub(super) fn nul_paths(out: &str) -> impl Iterator<Item = String> + '_ {
    out.split('\0').filter(|s| !s.is_empty()).map(str::to_string)
}

pub(super) fn plus_line_path(rest: &str) -> Option<String> {
    let path = decode_git_path(rest)?;
    let path = path.strip_prefix("b/")?;
    if path == "/dev/null" || path.is_empty() {
        return None;
    }
    Some(path.to_string())
}

fn decode_git_path(raw: &str) -> Option<String> {
    let raw = raw.strip_suffix('\r').unwrap_or(raw);
    if let Some(quoted) = raw.strip_prefix('"') {
        return unescape_git_quoted(quoted);
    }
    Some(raw.split('\t').next().unwrap_or(raw).to_string())
}

fn unescape_git_quoted(after_open: &str) -> Option<String> {
    let mut out = Vec::new();
    let bytes = after_open.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return String::from_utf8(out).ok(),
            b'\\' => {
                i += 1;
                let (byte, used) = unescape_one(bytes.get(i..)?)?;
                out.push(byte);
                i += used;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    None
}

fn unescape_one(rest: &[u8]) -> Option<(u8, usize)> {
    let first = *rest.first()?;
    if (b'0'..=b'7').contains(&first) {
        return octal_byte(rest);
    }
    Some((letter_escape(first).unwrap_or(first), 1))
}

fn octal_byte(rest: &[u8]) -> Option<(u8, usize)> {
    let digits = rest.get(..3)?;
    let mut val = 0u16;
    for &b in digits {
        if !(b'0'..=b'7').contains(&b) {
            return None;
        }
        val = val * 8 + u16::from(b - b'0');
    }
    u8::try_from(val).ok().map(|b| (b, 3))
}

fn letter_escape(b: u8) -> Option<u8> {
    Some(match b {
        b'n' => b'\n',
        b't' => b'\t',
        b'r' => b'\r',
        b'a' => 0x07,
        b'b' => 0x08,
        b'f' => 0x0c,
        b'v' => 0x0b,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plus_line_path_strips_tab_and_unquotes() {
        assert_eq!(
            plus_line_path("b/my file.py\t"),
            Some("my file.py".into())
        );
        assert_eq!(
            plus_line_path("\"b/tab\\tfile.py\""),
            Some("tab\tfile.py".into())
        );
        assert_eq!(
            plus_line_path("\"b/quote\\\"file.py\""),
            Some("quote\"file.py".into())
        );
        assert_eq!(
            plus_line_path("\"b/caf\\303\\251.py\""),
            Some("café.py".into())
        );
        assert_eq!(plus_line_path("b/src/lib.rs"), Some("src/lib.rs".into()));
        assert_eq!(plus_line_path("/dev/null"), None);
    }

    #[test]
    fn nul_paths_skips_empty_slots() {
        let got: Vec<String> = nul_paths("a.py\0b.py\0").collect();
        assert_eq!(got, vec!["a.py".to_string(), "b.py".to_string()]);
    }
}
