use std::collections::BTreeSet;

const MAX_STATEMENT_LINES: usize = 200;

fn rslip_fnv1a64(h: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(h, |acc, byte| {
        (acc ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[derive(Default)]
struct Scan {
    depth: i32,
    quote: Option<(char, bool)>,
}

impl Scan {
    fn feed_line(&mut self, line: &str) -> bool {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        let mut code_end = chars.len();
        while i < chars.len() {
            let c = chars[i];
            if let Some((q, triple)) = self.quote {
                i += self.step_in_string(&chars, i, q, triple);
                continue;
            }
            match c {
                '#' => {
                    code_end = i;
                    break;
                }
                '\'' | '"' => {
                    let triple = chars.get(i + 1) == Some(&c) && chars.get(i + 2) == Some(&c);
                    self.quote = Some((c, triple));
                    i += if triple { 3 } else { 1 };
                    continue;
                }
                '(' | '[' | '{' => self.depth += 1,
                ')' | ']' | '}' => self.depth = (self.depth - 1).max(0),
                _ => {}
            }
            i += 1;
        }
        if matches!(self.quote, Some((_, false))) {
            self.quote = None;
        }
        let backslash = chars[..code_end].iter().rev().find(|c| !c.is_whitespace()) == Some(&'\\');
        self.depth > 0 || self.quote.is_some() || backslash
    }

    fn step_in_string(&mut self, chars: &[char], i: usize, q: char, triple: bool) -> usize {
        if chars[i] == '\\' {
            return 2;
        }
        if chars[i] != q {
            return 1;
        }
        if !triple {
            self.quote = None;
            return 1;
        }
        if chars.get(i + 1) == Some(&q) && chars.get(i + 2) == Some(&q) {
            self.quote = None;
            return 3;
        }
        1
    }
}

fn add_statement(lines: &[&str], start: usize, keep: &mut BTreeSet<usize>) {
    let mut scan = Scan::default();
    for (idx, line) in lines
        .iter()
        .enumerate()
        .skip(start)
        .take(MAX_STATEMENT_LINES)
    {
        keep.insert(idx);
        if !scan.feed_line(line) {
            return;
        }
    }
}

fn indent_of(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    Some(line.len() - trimmed.len())
}

fn add_header_chain(lines: &[&str], idx: usize, keep: &mut BTreeSet<usize>) {
    let Some(mut current) = indent_of(lines[idx]) else {
        return;
    };
    for j in (0..idx).rev() {
        if current == 0 {
            return;
        }
        match indent_of(lines[j]) {
            Some(indent) if indent < current => {
                add_statement(lines, j, keep);
                current = indent;
            }
            _ => {}
        }
    }
}

fn statement_end(lines: &[&str], start: usize) -> usize {
    let mut scan = Scan::default();
    let mut idx = start;
    while idx < lines.len() && idx < start + MAX_STATEMENT_LINES {
        let continues = scan.feed_line(lines[idx]);
        idx += 1;
        if !continues {
            break;
        }
    }
    idx
}

fn is_def_header(trimmed: &str) -> bool {
    trimmed.starts_with("def ") || trimmed.starts_with("async def ")
}

fn add_import_time_statements(lines: &[&str], keep: &mut BTreeSet<usize>) {
    let mut def_indent: Option<usize> = None;
    let mut idx = 0;
    while idx < lines.len() {
        let Some(indent) = indent_of(lines[idx]) else {
            idx += 1;
            continue;
        };
        let end = statement_end(lines, idx);
        if def_indent.is_some_and(|d| indent > d) {
            idx = end;
            continue;
        }
        keep.extend(idx..end);
        def_indent = is_def_header(lines[idx].trim_start()).then_some(indent);
        idx = end;
    }
}

pub(crate) fn covered_statements_digest(text: &str, covered: &BTreeSet<u32>) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut keep = BTreeSet::new();
    add_import_time_statements(&lines, &mut keep);
    let mut h = rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b"covered-statements");
    for &line_no in covered {
        let idx = (line_no as usize).wrapping_sub(1);
        if idx >= lines.len() {
            h = rslip_fnv1a64(h, format!("missing:{line_no}\n").as_bytes());
            continue;
        }
        add_statement(&lines, idx, &mut keep);
        add_header_chain(&lines, idx, &mut keep);
    }
    for idx in keep {
        h = rslip_fnv1a64(h, format!("{idx}:").as_bytes());
        h = rslip_fnv1a64(h, lines[idx].as_bytes());
        h = rslip_fnv1a64(h, b"\n");
    }
    format!("{h:016x}")
}

#[cfg(test)]
#[path = "statement_digest_test.rs"]
mod statement_digest_test;
