use super::super::super::progress_watch_report::{strip_ansi, strip_trailing_duration};
use super::super::SuiteOutcome;

pub(super) enum ParsedWatchLine {
    Named {
        selector: String,
        outcome: SuiteOutcome,
    },
    Collapsed {
        outcome: SuiteOutcome,
        count: usize,
    },
    LangCollapsed {
        lang: crate::Language,
        outcome: SuiteOutcome,
        count: usize,
    },
    Summary {
        passed: usize,
        failed: usize,
        timed_out: usize,
        total: String,
        max_pass: String,
    },
    Violation(String),
    NoViolations,
    Ignore,
}

pub(super) fn parse_watch_line(message: &str) -> ParsedWatchLine {
    let line = strip_ansi(message.trim());
    let line = line.as_ref();
    if line.contains("VIOLATION:") {
        return ParsedWatchLine::Violation(line.to_string());
    }
    if line == "NO VIOLATIONS" {
        return ParsedWatchLine::NoViolations;
    }
    if let Some(parsed) = parse_lang_collapsed(line) {
        return parsed;
    }
    parse_summary_line(line)
        .unwrap_or_else(|| parse_status_line(line).unwrap_or(ParsedWatchLine::Ignore))
}

fn parse_lang_collapsed(line: &str) -> Option<ParsedWatchLine> {
    let rest = line.strip_prefix("kiss test: lang_collapsed ")?;
    let mut parts = rest.split_whitespace();
    let lang = match parts.next()? {
        "python" | "py" => crate::Language::Python,
        "rust" | "rs" => crate::Language::Rust,
        _ => return None,
    };
    let outcome = match parts.next()? {
        "pass" => SuiteOutcome::Pass,
        "fail" => SuiteOutcome::Fail,
        "timeout" => SuiteOutcome::Timeout,
        _ => return None,
    };
    let count = parts.next()?.parse().ok()?;
    Some(ParsedWatchLine::LangCollapsed {
        lang,
        outcome,
        count,
    })
}

fn parse_summary_line(line: &str) -> Option<ParsedWatchLine> {
    let rest = line
        .strip_prefix("✓ ")
        .or_else(|| line.strip_prefix("✗ "))?;
    if !rest.contains(" passed · ") {
        return None;
    }
    let parts: Vec<&str> = rest.split(" · ").collect();
    if parts.len() < 3 {
        return None;
    }
    Some(ParsedWatchLine::Summary {
        passed: parse_count_word(parts[0], "passed")?,
        failed: parse_count_word(parts[1], "failed")?,
        timed_out: parse_count_word(parts[2], "timed out")?,
        total: part_suffix(parts.get(3).copied(), " total"),
        max_pass: part_suffix(parts.get(4).copied(), " max pass"),
    })
}

fn parse_count_word(part: &str, word: &str) -> Option<usize> {
    part.strip_suffix(word)?.trim().parse().ok()
}

fn part_suffix(part: Option<&str>, suffix: &str) -> String {
    part.and_then(|text| text.strip_suffix(suffix))
        .unwrap_or("0s")
        .to_string()
}

fn parse_status_line(line: &str) -> Option<ParsedWatchLine> {
    let (outcome, rest) = if let Some(rest) = line.strip_prefix("PASS") {
        (SuiteOutcome::Pass, rest)
    } else if let Some(rest) = line.strip_prefix("TIMEOUT") {
        (SuiteOutcome::Timeout, rest)
    } else {
        (SuiteOutcome::Fail, line.strip_prefix("FAIL")?)
    };
    let body = rest
        .strip_prefix(" (cached): ")
        .or_else(|| rest.strip_prefix(": "))
        .or_else(|| rest.strip_prefix(' '))?;
    let selector = strip_trailing_duration(body);
    if let Some(count) = selector
        .strip_suffix(" selectors")
        .and_then(|n| n.parse::<usize>().ok())
    {
        return Some(ParsedWatchLine::Collapsed { outcome, count });
    }
    if selector.is_empty() {
        return None;
    }
    Some(ParsedWatchLine::Named {
        selector: selector.to_string(),
        outcome,
    })
}
