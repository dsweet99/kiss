use std::path::Path;

use crate::config::{ConfigError, check_unknown_keys, parse_string_list_key};

pub const CACHE_POLICY_SCHEMA_VERSION: &str =
    crate::rust_llvm_cov_runner::CACHE_POLICY_SCHEMA_VERSION;

const CACHE_KEYS: &[&str] = &["non_cacheable", "inputs"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestCachePolicy {
    pub non_cacheable: Vec<String>,
    pub inputs: Vec<TestCacheInput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestCacheInput {
    pub selectors: Vec<String>,
    pub paths: Vec<String>,
}

impl TestCachePolicy {
    pub fn parse_table(table: &toml::Table, repo_root: Option<&Path>) -> Result<Self, ConfigError> {
        check_unknown_keys(table, CACHE_KEYS, "test.cache")?;
        let mut policy = Self::default();
        if let Some(value) = table.get("non_cacheable") {
            policy.non_cacheable =
                parse_string_list_key(value, "non_cacheable", "selector patterns")?;
        }
        if let Some(inputs) = table.get("inputs") {
            policy.inputs = parse_inputs(inputs, repo_root)?;
        }
        Ok(policy)
    }

    pub fn is_non_cacheable(&self, selector: &str) -> bool {
        self.non_cacheable
            .iter()
            .any(|pattern| selector_matches(pattern, selector))
    }

    pub fn declared_paths(&self, selector: &str) -> Vec<String> {
        let mut paths = Vec::new();
        for input in &self.inputs {
            if input
                .selectors
                .iter()
                .any(|pattern| selector_matches(pattern, selector))
            {
                paths.extend(input.paths.iter().cloned());
            }
        }
        paths.sort();
        paths.dedup();
        paths
    }

    pub fn effective_digest(&self, selector: &str) -> String {
        format!(
            "{}:{}",
            self.is_non_cacheable(selector),
            self.declared_paths(selector).join(",")
        )
    }

    pub fn digest(&self) -> String {
        format!(
            "{CACHE_POLICY_SCHEMA_VERSION}:{}:{}",
            self.non_cacheable.join(","),
            self.inputs.len()
        )
    }
}

pub fn merge_language_adapters(repo_root: &Path, policy: &mut TestCachePolicy) {
    for pattern in adapter_patterns(repo_root) {
        if !policy.non_cacheable.iter().any(|existing| existing == &pattern) {
            policy.non_cacheable.push(pattern);
        }
    }
}

fn adapter_patterns(repo_root: &Path) -> Vec<String> {
    let mut patterns = Vec::new();
    for name in ["pytest.ini", "pyproject.toml", "setup.cfg", "tox.ini"] {
        let Ok(text) = std::fs::read_to_string(repo_root.join(name)) else {
            continue;
        };
        if text.contains("kiss_non_cacheable") {
            patterns.push("*kiss_non_cacheable*".into());
        }
    }
    for name in [".config/nextest.toml", ".nextest/config.toml"] {
        let Ok(text) = std::fs::read_to_string(repo_root.join(name)) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("kiss-non-cacheable") {
                for part in rest
                    .trim_start_matches(['=', ':', ' '])
                    .trim_matches(['[', ']', '"', '\''])
                    .split(',')
                {
                    let part = part.trim().trim_matches(['"', '\'']);
                    if !part.is_empty() {
                        patterns.push(part.to_string());
                    }
                }
            }
        }
    }
    patterns.sort();
    patterns.dedup();
    patterns
}

fn selector_matches(pattern: &str, selector: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        return selector.starts_with(prefix);
    }
    selector == pattern
}

fn parse_inputs(
    value: &toml::Value,
    repo_root: Option<&Path>,
) -> Result<Vec<TestCacheInput>, ConfigError> {
    let Some(items) = value.as_array() else {
        return Err(ConfigError::InvalidValue {
            key: "test.cache.inputs".into(),
            message: "must be an array of tables".into(),
        });
    };
    let mut out = Vec::new();
    for item in items {
        let Some(table) = item.as_table() else {
            return Err(ConfigError::InvalidValue {
                key: "test.cache.inputs".into(),
                message: "each input must be a table".into(),
            });
        };
        check_unknown_keys(table, &["selectors", "paths"], "test.cache.inputs")?;
        let selectors = match table.get("selectors") {
            Some(value) => parse_string_list_key(value, "selectors", "selector patterns")?,
            None => Vec::new(),
        };
        let paths = match table.get("paths") {
            Some(value) => parse_string_list_key(value, "paths", "paths")?,
            None => Vec::new(),
        };
        reject_out_of_scope_paths(repo_root, &paths)?;
        out.push(TestCacheInput { selectors, paths });
    }
    Ok(out)
}

fn reject_out_of_scope_paths(
    repo_root: Option<&Path>,
    paths: &[String],
) -> Result<(), ConfigError> {
    for path in paths {
        if path_escapes_repo(path) {
            return Err(ConfigError::InvalidValue {
                key: "test.cache.inputs.paths".into(),
                message: format!("declared data path is outside the repository: {path}"),
            });
        }
        if let Some(root) = repo_root {
            let joined = root.join(path);
            let canonical = joined.canonicalize().unwrap_or(joined);
            let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
            if !canonical.starts_with(&root) {
                return Err(ConfigError::InvalidValue {
                    key: "test.cache.inputs.paths".into(),
                    message: format!("declared data path is outside the repository: {path}"),
                });
            }
        }
    }
    Ok(())
}

fn path_escapes_repo(path: &str) -> bool {
    let p = Path::new(path);
    if p.is_absolute() {
        return true;
    }
    let mut depth = 0i32;
    for component in p.components() {
        match component {
            std::path::Component::ParentDir => depth -= 1,
            std::path::Component::Normal(_) => depth += 1,
            std::path::Component::CurDir => {}
            _ => return true,
        }
        if depth < 0 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_non_cacheable_and_rejects_outside_paths() {
        let table: toml::Table = r#"
non_cacheable = ["flaky::*"]
[[inputs]]
selectors = ["data::*"]
paths = ["fixtures/data.json"]
"#
        .parse()
        .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("fixtures")).unwrap();
        std::fs::write(tmp.path().join("fixtures").join("data.json"), "{}").unwrap();
        let policy = TestCachePolicy::parse_table(&table, Some(tmp.path())).unwrap();
        assert!(policy.is_non_cacheable("flaky::one"));
        assert!(!policy.is_non_cacheable("stable::one"));
        assert_eq!(policy.inputs[0].paths, ["fixtures/data.json"]);

        let bad: toml::Table = r#"
[[inputs]]
selectors = ["x"]
paths = ["/etc/passwd"]
"#
        .parse()
        .unwrap();
        assert!(TestCachePolicy::parse_table(&bad, Some(tmp.path())).is_err());
        assert!(TestCachePolicy::parse_table(&bad, None).is_err());
    }
}
