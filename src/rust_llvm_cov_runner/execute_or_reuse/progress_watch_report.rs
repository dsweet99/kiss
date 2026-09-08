use std::sync::{Mutex, OnceLock};

fn watch_report_lines() -> &'static Mutex<Option<Vec<String>>> {
    static LINES: OnceLock<Mutex<Option<Vec<String>>>> = OnceLock::new();
    LINES.get_or_init(|| Mutex::new(None))
}

pub fn begin_watch_report_capture() {
    *watch_report_lines()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Vec::new());
}

#[must_use]
pub fn take_watch_report_lines() -> Option<Vec<String>> {
    watch_report_lines()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

#[must_use]
pub fn take_watch_report_capture() -> Option<String> {
    Some(compact_watch_report(&take_watch_report_lines()?))
}

pub(crate) fn record_watch_report_line(message: &str) {
    let mut slot = watch_report_lines()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(lines) = slot.as_mut() else {
        return;
    };
    if is_watch_report_line(message) {
        lines.push(message.to_string());
    }
}

pub(crate) fn strip_ansi_prefix(message: &str) -> &str {
    let mut rest = message;
    while let Some(stripped) = rest.strip_prefix("\x1b[") {
        rest = stripped.split_once('m').map_or(rest, |(_, tail)| tail);
    }
    rest
}

pub fn transcript_from_lines(lines: &[String]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let full = lines.join("\n");
    if full.len() <= WATCH_REPORT_BUDGET {
        Some(full)
    } else {
        Some(compact_watch_report(lines))
    }
}

fn is_watch_report_line(message: &str) -> bool {
    !strip_ansi_prefix(message.trim()).is_empty()
}

const WATCH_REPORT_BUDGET: usize = 200 * 1024;

fn compact_watch_report(lines: &[String]) -> String {
    let full = lines.join("\n");
    if full.len() <= WATCH_REPORT_BUDGET {
        return full;
    }
    let kept: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|line| {
            let text = strip_ansi_prefix(line.trim());
            !text.starts_with("PASS")
        })
        .collect();
    let compact = kept.join("\n");
    if compact.len() <= WATCH_REPORT_BUDGET {
        return compact;
    }
    let mut end = WATCH_REPORT_BUDGET.min(compact.len());
    while end > 0 && !compact.is_char_boundary(end) {
        end -= 1;
    }
    compact[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_report_capture_keeps_status_and_summary() {
        begin_watch_report_capture();
        record_watch_report_line("kiss test: Starting");
        record_watch_report_line("kiss test: request force=false force_bad=false metrics=false");
        record_watch_report_line("kiss test: Planning ...");
        record_watch_report_line("PASS: tests/a.py::t (0.01s)");
        record_watch_report_line("TIMEOUT: tests/b.py::t (5.00s)");
        record_watch_report_line("✓ 1 passed · 0 failed · 1 timed out · 1s total · 0s max pass");
        let report = take_watch_report_capture().expect("captured");
        assert!(report.contains("PASS: tests/a.py::t"));
        assert!(report.contains("TIMEOUT: tests/b.py::t"));
        assert!(report.contains("passed ·"));
        assert!(
            report.contains("Planning"),
            "client transcript must keep the standalone progress lines; report={report:?}"
        );
        assert!(report.contains("Starting"));
        assert!(report.contains("request"));
        assert!(take_watch_report_capture().is_none());
    }
}
