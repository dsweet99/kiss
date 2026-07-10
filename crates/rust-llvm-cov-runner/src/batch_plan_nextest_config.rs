use crate::batch_plan::RustCoverageBatchRequest;

pub(crate) fn build_nextest_config_toml(req: &RustCoverageBatchRequest) -> String {
    let default_filter = build_nextest_default_filter(req);
    format!(
        "[profile.kiss]\ndefault-filter = {}\nretries = 0\nfail-fast = false\n",
        toml_basic_string(&default_filter)
    )
}

fn build_nextest_default_filter(req: &RustCoverageBatchRequest) -> String {
    let operator = if rust_test_args_request_exact_match(&req.test_args) {
        "="
    } else {
        "~"
    };
    req.logical_selectors
        .iter()
        .map(|selector| format!("test({operator}{})", nextest_filter_string(selector)))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn rust_test_args_request_exact_match(test_args: &[String]) -> bool {
    test_args.iter().any(|arg| arg == "--exact")
}

fn nextest_filter_string(value: &str) -> String {
    format!("\"{}\"", escape_nextest_filter_string(value))
}

fn escape_nextest_filter_string(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| ch.escape_default())
        .collect::<String>()
}

fn toml_basic_string(value: &str) -> String {
    format!("\"{}\"", escape_nextest_filter_string(value))
}

#[cfg(test)]
mod tests {
    use super::{escape_nextest_filter_string, nextest_filter_string, toml_basic_string};

    #[test]
    fn escaping_preserves_nextest_and_toml_string_boundaries() {
        let value = "quote\"slash\\line\n";

        assert_eq!(
            escape_nextest_filter_string(value),
            r#"quote\"slash\\line\n"#
        );
        assert_eq!(nextest_filter_string(value), r#""quote\"slash\\line\n""#);
        assert_eq!(toml_basic_string(value), r#""quote\"slash\\line\n""#);
    }
}
