use super::KissTestReport;

pub(super) fn replay_suite_report(report: &KissTestReport) {
    let Some(output) = report.output.as_deref() else {
        return;
    };
    if output.is_empty() {
        return;
    }
    print!("{output}");
    if !output.ends_with('\n') {
        println!();
    }
}

#[cfg(test)]
#[path = "suite_report_test.rs"]
mod tests;
