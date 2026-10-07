use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListedTest {
    pub(crate) binary_id: String,
    pub(crate) name: String,
}

pub(crate) fn list_tests(repo_root: &Path) -> Result<Vec<ListedTest>, String> {
    let output = Command::new("cargo")
        .args([
            "nextest",
            "list",
            "--workspace",
            "--message-format",
            "json",
            "--color",
            "never",
            "--cargo-quiet",
            "--user-config-file",
            "none",
        ])
        .current_dir(repo_root)
        .env_clear()
        .envs(super::env::child_env())
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("error: kiss test: failed to run cargo nextest list: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "error: kiss test: cargo nextest list failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    parse_list(&output.stdout)
}

fn parse_list(stdout: &[u8]) -> Result<Vec<ListedTest>, String> {
    let value: serde_json::Value = serde_json::from_slice(stdout)
        .map_err(|err| format!("error: kiss test: unreadable cargo nextest list output: {err}"))?;
    let suites = value
        .get("rust-suites")
        .and_then(serde_json::Value::as_object)
        .ok_or("error: kiss test: cargo nextest list output has no rust-suites")?;
    let mut tests = Vec::new();
    for (binary_id, suite) in suites {
        let Some(cases) = suite
            .get("testcases")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        for (name, case) in cases {
            if case
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("test")
                == "test"
            {
                tests.push(ListedTest {
                    binary_id: binary_id.clone(),
                    name: name.clone(),
                });
            }
        }
    }
    Ok(tests)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_output_yields_each_test_with_its_binary() {
        let json = br#"{"test-count":3,"rust-suites":{
            "pkg":{"binary-id":"pkg","testcases":{
                "t::a":{"kind":"test","ignored":false},
                "b::bench":{"kind":"benchmark","ignored":false}}},
            "pkg::bin/tool":{"binary-id":"pkg::bin/tool","testcases":{
                "t::b":{"ignored":true}}}}}"#;
        let tests = parse_list(json).unwrap();
        assert_eq!(
            tests,
            [
                ListedTest {
                    binary_id: "pkg".into(),
                    name: "t::a".into()
                },
                ListedTest {
                    binary_id: "pkg::bin/tool".into(),
                    name: "t::b".into()
                },
            ]
        );
        assert!(parse_list(b"{}").is_err());
        assert!(parse_list(b"not json").is_err());
    }
}
