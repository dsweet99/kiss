use super::{
    WatchNamedOutcome, WatchReportCapture, progress_lang, strip_ansi, strip_trailing_duration,
};

pub(super) fn apply(capture: &mut WatchReportCapture, message: &str) {
    let Some(lang) = progress_lang() else {
        return;
    };
    let line = strip_ansi(message.trim());
    if line.starts_with("kiss test: lang_collapsed ") {
        return;
    }
    let Some((outcome, body)) = status_body(line.as_ref()) else {
        return;
    };
    let selector = strip_trailing_duration(body);
    if let Some(count) = selector
        .strip_suffix(" selectors")
        .and_then(|n| n.parse::<usize>().ok())
    {
        add_count(capture, lang, outcome, count);
        return;
    }
    if selector.is_empty() {
        return;
    }
    insert_named(capture, lang, selector, outcome);
}

fn status_body(line: &str) -> Option<(WatchNamedOutcome, &str)> {
    let (outcome, rest) = if let Some(rest) = line.strip_prefix("PASS") {
        (WatchNamedOutcome::Pass, rest)
    } else if let Some(rest) = line.strip_prefix("TIMEOUT") {
        (WatchNamedOutcome::Timeout, rest)
    } else {
        (WatchNamedOutcome::Fail, line.strip_prefix("FAIL")?)
    };
    rest.strip_prefix(" (cached): ")
        .or_else(|| rest.strip_prefix(": "))
        .map(|body| (outcome, body))
}

fn insert_named(
    capture: &mut WatchReportCapture,
    lang: crate::Language,
    selector: &str,
    outcome: WatchNamedOutcome,
) {
    match capture.named.insert((lang, selector.to_string()), outcome) {
        None => add_count(capture, lang, outcome, 1),
        Some(old) if old != outcome => {
            dec_count(capture, lang, old);
            add_count(capture, lang, outcome, 1);
        }
        Some(_) => {}
    }
}

fn add_count(
    capture: &mut WatchReportCapture,
    lang: crate::Language,
    outcome: WatchNamedOutcome,
    count: usize,
) {
    *slot_mut(capture, lang, outcome) += count;
}

fn dec_count(
    capture: &mut WatchReportCapture,
    lang: crate::Language,
    outcome: WatchNamedOutcome,
) {
    let slot = slot_mut(capture, lang, outcome);
    *slot = slot.saturating_sub(1);
}

fn slot_mut(
    capture: &mut WatchReportCapture,
    lang: crate::Language,
    outcome: WatchNamedOutcome,
) -> &mut usize {
    let i = match lang {
        crate::Language::Python => 0,
        crate::Language::Rust => 1,
    };
    match outcome {
        WatchNamedOutcome::Pass => &mut capture.lang_passed[i],
        WatchNamedOutcome::Fail => &mut capture.lang_failed[i],
        WatchNamedOutcome::Timeout => &mut capture.lang_timed_out[i],
    }
}
