use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use super::status_line::FinishedTest;

#[derive(Default)]
pub(super) struct TestNames {
    binary_ids: HashMap<String, Vec<String>>,
    exact: HashMap<String, Vec<(String, String)>>,
}

impl TestNames {
    pub(super) fn resolve(
        repo_root: &Path,
        selectors: &[String],
        report_ids: &BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let read_err = |err| format!("error: kiss test: read Cargo targets: {err}");
        let binary_ids =
            kiss::code_roles::workspace_nextest_binary_ids(repo_root).map_err(read_err)?;
        let files =
            kiss::code_roles::workspace_nextest_file_modules(repo_root).map_err(read_err)?;
        let exact = selectors
            .iter()
            .filter_map(|selector| {
                let names = match selector.split_once('$') {
                    Some((prefix, path)) => binary_ids
                        .get(prefix)
                        .into_iter()
                        .flatten()
                        .map(|binary_id| (binary_id.clone(), path.to_string()))
                        .collect(),
                    None => file_names(repo_root, selector, report_ids.get(selector)?, &files),
                };
                (!names.is_empty()).then(|| (selector.clone(), names))
            })
            .collect();
        Ok(Self { binary_ids, exact })
    }
}

fn file_names(
    repo_root: &Path,
    test_path: &str,
    report_id: &str,
    files: &HashMap<std::path::PathBuf, Vec<(String, String)>>,
) -> Vec<(String, String)> {
    let Some(end) = report_id.find(".rs::") else {
        return Vec::new();
    };
    let file = kiss::rust_include::canonical_path(&repo_root.join(&report_id[..end + 3]));
    files
        .get(&file)
        .into_iter()
        .flatten()
        .map(|(binary_id, module)| {
            let name = if module.is_empty() {
                test_path.to_string()
            } else {
                format!("{module}::{test_path}")
            };
            (binary_id.clone(), name)
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TestTarget {
    exact: Vec<(String, String)>,
    binary_ids: Vec<String>,
    test_path: String,
}

fn test_target(selector: &str, names: &TestNames) -> TestTarget {
    let exact = names.exact.get(selector).cloned().unwrap_or_default();
    match selector.split_once('$') {
        Some((prefix, test_path)) => TestTarget {
            exact,
            binary_ids: names.binary_ids.get(prefix).cloned().unwrap_or_default(),
            test_path: test_path.to_string(),
        },
        None => TestTarget {
            exact,
            binary_ids: Vec::new(),
            test_path: selector.to_string(),
        },
    }
}

fn escape_regex(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        if matches!(
            ch,
            '\\' | '.'
                | '^'
                | '$'
                | '|'
                | '?'
                | '*'
                | '+'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '/'
        ) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

fn target_filter(target: &TestTarget) -> String {
    if !target.exact.is_empty() {
        let parts: Vec<String> = target
            .exact
            .iter()
            .map(|(binary_id, name)| format!("(binary_id(={binary_id}) & test(={name}))"))
            .collect();
        return parts.join(" | ");
    }
    let test = format!("test(/(^|::){}$/)", escape_regex(&target.test_path));
    let binaries: Vec<String> = target
        .binary_ids
        .iter()
        .map(|binary_id| format!("binary_id(={binary_id})"))
        .collect();
    match binaries.as_slice() {
        [] => test,
        [one] => format!("({one} & {test})"),
        many => format!("(({}) & {test})", many.join(" | ")),
    }
}

pub(super) fn selectors_filter<'a>(
    selectors: impl IntoIterator<Item = &'a String>,
    names: &TestNames,
) -> String {
    let parts: Vec<String> = selectors
        .into_iter()
        .map(|selector| target_filter(&test_target(selector, names)))
        .collect();
    if parts.is_empty() {
        "none()".to_string()
    } else {
        parts.join(" | ")
    }
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

fn format_period(millis: u64) -> String {
    let millis = millis.max(1);
    if millis.is_multiple_of(1000) {
        format!("{}s", millis / 1000)
    } else {
        format!("{millis}ms")
    }
}

fn slow_timeout(millis: u64) -> String {
    format!(
        "slow-timeout = {{ period = {}, terminate-after = 1 }}\n",
        toml_string(&format_period(millis))
    )
}

pub(super) fn tool_config_toml(
    selectors: &[String],
    names: &TestNames,
    timeout_millis: &BTreeMap<String, u64>,
) -> String {
    let mut out = format!(
        "[profile.kiss]\ndefault-filter = {}\nretries = 0\nfail-fast = false\n",
        toml_string(&selectors_filter(selectors, names))
    );
    let mut by_limit: BTreeMap<u64, Vec<&String>> = BTreeMap::new();
    for selector in selectors {
        if let Some(&millis) = timeout_millis.get(selector) {
            by_limit.entry(millis).or_default().push(selector);
        }
    }
    let common = by_limit
        .iter()
        .max_by_key(|(millis, group)| (group.len(), std::cmp::Reverse(**millis)))
        .map(|(millis, _)| *millis);
    if let Some(millis) = common.filter(|_| timeout_millis.len() == selectors.len()) {
        out.push_str(&slow_timeout(millis));
        by_limit.remove(&millis);
    }
    for (millis, group) in by_limit {
        out.push_str("\n[[profile.kiss.overrides]]\n");
        out.push_str(&format!(
            "filter = {}\n",
            toml_string(&selectors_filter(group, names))
        ));
        out.push_str(&slow_timeout(millis));
    }
    out
}

pub(super) struct SelectorIndex {
    exact: HashMap<(String, String), Vec<String>>,
    by_test_path: HashMap<String, Vec<(String, Vec<String>)>>,
}

impl SelectorIndex {
    pub(super) fn new(selectors: &[String], names: &TestNames) -> Self {
        let mut exact: HashMap<(String, String), Vec<String>> = HashMap::new();
        let mut by_test_path: HashMap<String, Vec<(String, Vec<String>)>> = HashMap::new();
        for selector in selectors {
            let target = test_target(selector, names);
            if !target.exact.is_empty() {
                for key in target.exact {
                    exact.entry(key).or_default().push(selector.clone());
                }
                continue;
            }
            by_test_path
                .entry(target.test_path)
                .or_default()
                .push((selector.clone(), target.binary_ids));
        }
        Self {
            exact,
            by_test_path,
        }
    }

    pub(super) fn selectors_for(&self, finished: &FinishedTest) -> Vec<&str> {
        let key = (finished.binary_id.clone(), finished.test_name.clone());
        if let Some(found) = self.exact.get(&key) {
            return found.iter().map(String::as_str).collect();
        }
        let name = finished.test_name.as_str();
        std::iter::once(name)
            .chain(name.match_indices("::").map(|(at, _)| &name[at + 2..]))
            .map(|path| self.matching(path, &finished.binary_id))
            .find(|found| !found.is_empty())
            .unwrap_or_default()
    }

    fn matching(&self, test_path: &str, binary_id: &str) -> Vec<&str> {
        self.by_test_path
            .get(test_path)
            .into_iter()
            .flatten()
            .filter(|(_, wanted)| wanted.is_empty() || wanted.iter().any(|id| id == binary_id))
            .map(|(selector, _)| selector.as_str())
            .collect()
    }
}

pub(super) fn test_threads(repo_root: &Path, extras: &[String], jobs: usize) -> usize {
    if extras
        .iter()
        .any(|arg| matches!(arg.as_str(), "--nocapture" | "--no-capture"))
    {
        return 1;
    }
    let config =
        kiss::TestSectionConfig::try_load_path_only(&kiss::kissconfig_path_for_repo(repo_root))
            .unwrap_or_default();
    config
        .num_jobs_nextest_explicit
        .or_else(|| repo_default_test_threads(repo_root))
        .unwrap_or(jobs)
        .max(1)
}

fn repo_default_test_threads(repo_root: &Path) -> Option<usize> {
    [".config/nextest.toml", "nextest.toml"]
        .iter()
        .find_map(|rel| {
            let table = std::fs::read_to_string(repo_root.join(rel))
                .ok()?
                .parse::<toml::Table>()
                .ok()?;
            let threads = table.get("profile")?.get("default")?.get("test-threads")?;
            usize::try_from(threads.as_integer()?)
                .ok()
                .filter(|n| *n > 0)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiss::rpytest_runner::TestStatus;
    use std::time::Duration;

    fn ids() -> TestNames {
        TestNames {
            binary_ids: HashMap::from([
                ("pkg::pkg_lib".to_string(), vec!["pkg".to_string()]),
                ("pkg::tool".to_string(), vec!["pkg::bin/tool".to_string()]),
                (
                    "pkg::pkg".to_string(),
                    vec!["pkg".to_string(), "pkg::bin/pkg".to_string()],
                ),
            ]),
            exact: HashMap::new(),
        }
    }

    fn finished(binary_id: &str, test_name: &str) -> FinishedTest {
        FinishedTest {
            status: TestStatus::Passed,
            duration: Duration::ZERO,
            binary_id: binary_id.into(),
            test_name: test_name.into(),
        }
    }

    #[test]
    fn filter_matches_each_selector_as_a_path_suffix() {
        let selectors = vec![
            "tests::bare".to_string(),
            "pkg::tool$tests::same".to_string(),
            "$tests::shared".to_string(),
        ];
        let toml = tool_config_toml(&selectors, &ids(), &BTreeMap::new());
        let table: toml::Table = toml.parse().unwrap();
        let filter = table["profile"]["kiss"]["default-filter"].as_str().unwrap();
        assert_eq!(
            filter,
            "test(/(^|::)tests::bare$/) | (binary_id(=pkg::bin/tool) & test(/(^|::)tests::same$/)) | test(/(^|::)tests::shared$/)"
        );
        assert!(!toml.contains("slow-timeout"), "{toml}");
        let empty = tool_config_toml(&[], &ids(), &BTreeMap::new());
        assert!(empty.contains("default-filter = \"none()\""), "{empty}");
    }

    #[test]
    fn shared_limit_goes_on_the_profile_and_others_become_overrides() {
        let selectors = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let limits = BTreeMap::from([
            ("a".to_string(), 3000),
            ("b".to_string(), 3000),
            ("c".to_string(), 1500),
        ]);
        let toml = tool_config_toml(&selectors, &ids(), &limits);
        let table: toml::Table = toml.parse().unwrap();
        let profile = &table["profile"]["kiss"];
        assert_eq!(profile["slow-timeout"]["period"].as_str(), Some("3s"));
        assert_eq!(
            profile["slow-timeout"]["terminate-after"].as_integer(),
            Some(1)
        );
        let overrides = profile["overrides"].as_array().unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0]["filter"].as_str(), Some("test(/(^|::)c$/)"));
        assert_eq!(
            overrides[0]["slow-timeout"]["period"].as_str(),
            Some("1500ms")
        );
    }

    #[test]
    fn unbounded_selectors_keep_the_profile_without_a_limit() {
        let selectors = vec!["a".to_string(), "b".to_string()];
        let limits = BTreeMap::from([("a".to_string(), 2000)]);
        let toml = tool_config_toml(&selectors, &ids(), &limits);
        let table: toml::Table = toml.parse().unwrap();
        assert!(
            table["profile"]["kiss"].get("slow-timeout").is_none(),
            "{toml}"
        );
        assert_eq!(
            table["profile"]["kiss"]["overrides"][0]["filter"].as_str(),
            Some("test(/(^|::)a$/)")
        );
    }

    #[test]
    fn index_maps_results_to_bare_and_qualified_selectors() {
        let selectors = vec![
            "tests::bare".to_string(),
            "pkg::tool$tests::same".to_string(),
            "pkg::pkg_lib$tests::same".to_string(),
        ];
        let index = SelectorIndex::new(&selectors, &ids());
        assert_eq!(
            index.selectors_for(&finished("pkg", "tests::bare")),
            ["tests::bare"]
        );
        assert_eq!(
            index.selectors_for(&finished("pkg::bin/tool", "tests::same")),
            ["pkg::tool$tests::same"]
        );
        assert_eq!(
            index.selectors_for(&finished("pkg", "tests::same")),
            ["pkg::pkg_lib$tests::same"]
        );
        assert!(
            index
                .selectors_for(&finished("pkg", "tests::other"))
                .is_empty()
        );
    }

    #[test]
    fn index_maps_module_qualified_names_to_the_longest_file_relative_selector() {
        let selectors = vec![
            "ok".to_string(),
            "t::ok".to_string(),
            "case_name".to_string(),
        ];
        let index = SelectorIndex::new(&selectors, &ids());
        assert_eq!(
            index.selectors_for(&finished("pkg", "runner::decision::tests::case_name")),
            ["case_name"]
        );
        assert_eq!(index.selectors_for(&finished("pkg", "a::t::ok")), ["t::ok"]);
        assert_eq!(index.selectors_for(&finished("pkg::it", "ok")), ["ok"]);
        assert!(
            index
                .selectors_for(&finished("pkg", "a::not_case_name"))
                .is_empty()
        );
    }

    #[test]
    fn lib_and_bin_sharing_a_name_both_match_the_shared_prefix() {
        let selectors = vec!["pkg::pkg$m::tests::t".to_string()];
        let filter = selectors_filter(&selectors, &ids());
        assert_eq!(
            filter,
            "((binary_id(=pkg) | binary_id(=pkg::bin/pkg)) & test(/(^|::)m::tests::t$/))"
        );
        let index = SelectorIndex::new(&selectors, &ids());
        for binary in ["pkg", "pkg::bin/pkg"] {
            assert_eq!(
                index.selectors_for(&finished(binary, "m::tests::t")),
                ["pkg::pkg$m::tests::t"]
            );
        }
        assert!(
            index
                .selectors_for(&finished("pkg::it", "m::tests::t"))
                .is_empty()
        );
    }

    #[test]
    fn resolved_names_are_exact_per_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src/a")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "mod a;\n").unwrap();
        std::fs::write(
            root.join("src/a.rs"),
            "#[cfg(test)]\n#[path = \"a/checks.rs\"]\nmod tests;\n",
        )
        .unwrap();
        std::fs::write(root.join("src/a/checks.rs"), "#[test]\nfn case() {}\n").unwrap();
        let selectors = vec!["case".to_string(), "pkg::pkg$a::tests::case".to_string()];
        let report_ids =
            BTreeMap::from([("case".to_string(), "src/a/checks.rs::case".to_string())]);
        let names = TestNames::resolve(root, &selectors, &report_ids).unwrap();
        let filter = selectors_filter(&selectors, &names);
        assert_eq!(
            filter,
            "(binary_id(=pkg) & test(=a::tests::case)) | (binary_id(=pkg) & test(=a::tests::case))"
        );
        let index = SelectorIndex::new(&selectors, &names);
        assert_eq!(
            index.selectors_for(&finished("pkg", "a::tests::case")),
            ["case", "pkg::pkg$a::tests::case"]
        );
        assert!(
            index
                .selectors_for(&finished("pkg", "b::tests::case"))
                .is_empty()
        );
    }

    #[test]
    fn exact_names_win_over_a_shorter_suffix_selector() {
        let mut names = ids();
        names.exact.insert(
            "test_x".to_string(),
            vec![("pkg".to_string(), "counts::tests::test_x".to_string())],
        );
        names.exact.insert(
            "tests::test_x".to_string(),
            vec![("pkg".to_string(), "violation::tests::test_x".to_string())],
        );
        let selectors = vec!["test_x".to_string(), "tests::test_x".to_string()];
        let index = SelectorIndex::new(&selectors, &names);
        assert_eq!(
            index.selectors_for(&finished("pkg", "counts::tests::test_x")),
            ["test_x"]
        );
        assert_eq!(
            index.selectors_for(&finished("pkg", "violation::tests::test_x")),
            ["tests::test_x"]
        );
    }

    #[test]
    fn filter_escapes_regex_metacharacters() {
        let toml = tool_config_toml(&["m::r#type".to_string()], &ids(), &BTreeMap::new());
        assert!(toml.contains("m::r#type$/"), "{toml}");
        assert_eq!(escape_regex("a.b(c)/d"), "a\\.b\\(c\\)\\/d");
    }

    #[test]
    fn threads_follow_nocapture_then_config_then_repo_then_jobs() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(test_threads(tmp.path(), &["--nocapture".into()], 8), 1);
        assert_eq!(test_threads(tmp.path(), &[], 8), 8);
        std::fs::create_dir_all(tmp.path().join(".config")).unwrap();
        std::fs::write(
            tmp.path().join(".config/nextest.toml"),
            "[profile.default]\ntest-threads = 2\n",
        )
        .unwrap();
        assert_eq!(test_threads(tmp.path(), &[], 8), 2);
        std::fs::write(
            tmp.path().join(".kissconfig"),
            "[rust]\nnum_jobs_nextest = 3\n",
        )
        .unwrap();
        assert_eq!(test_threads(tmp.path(), &[], 8), 3);
    }
}
