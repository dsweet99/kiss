use std::path::Path;

use super::fnmatch::fnmatch_ex;

const PYTEST_CONFIG_NAMES: [&str; 7] = [
    "pytest.toml",
    ".pytest.toml",
    "pytest.ini",
    ".pytest.ini",
    "pyproject.toml",
    "tox.ini",
    "setup.cfg",
];

pub(super) fn configured_python_filename_patterns(repo_root: &Path) -> Option<Vec<String>> {
    configured_python_filename_patterns_between(repo_root, repo_root)
}

pub(super) fn configured_python_filename_patterns_between(
    start: &Path,
    stop: &Path,
) -> Option<Vec<String>> {
    pytest_config_value(start, stop, python_files_from_config).flatten()
}

fn pytest_config_value<T>(
    start: &Path,
    stop: &Path,
    load: fn(&Path) -> Option<Option<T>>,
) -> Option<Option<T>> {
    let stop = stop.canonicalize().unwrap_or_else(|_| stop.to_path_buf());
    let mut current = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    if current != stop && !current.starts_with(&stop) {
        current = stop.clone();
    }
    loop {
        for name in PYTEST_CONFIG_NAMES {
            let path = current.join(name);
            if !path.is_file() {
                continue;
            }
            if let Some(value) = load(&path) {
                return Some(value);
            }
        }
        if current == stop {
            return None;
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => return None,
        }
    }
}

fn python_files_from_config(path: &Path) -> Option<Option<Vec<String>>> {
    let name = path.file_name()?.to_str()?;
    let text = std::fs::read_to_string(path).ok()?;
    match name {
        "pytest.toml" | ".pytest.toml" => Some(toml_table_python_files(&text, "pytest")),
        "pyproject.toml" => pyproject_config_patterns(&text),
        "pytest.ini" | ".pytest.ini" => Some(ini_python_files(&text, "[pytest]")),
        "tox.ini" => {
            ini_section_present(&text, "[pytest]").then(|| ini_python_files(&text, "[pytest]"))
        }
        "setup.cfg" => ini_section_present(&text, "[tool:pytest]")
            .then(|| ini_python_files(&text, "[tool:pytest]")),
        _ => None,
    }
}

fn pyproject_config_patterns(text: &str) -> Option<Option<Vec<String>>> {
    if !pyproject_is_pytest_config(text) {
        return None;
    }
    Some(pyproject_python_files(text))
}

fn pyproject_is_pytest_config(text: &str) -> bool {
    let Ok(value) = toml::from_str::<toml::Value>(text) else {
        return false;
    };
    value
        .get("tool")
        .and_then(|tool| tool.get("pytest"))
        .is_some()
}

fn pyproject_python_files(text: &str) -> Option<Vec<String>> {
    let value: toml::Value = toml::from_str(text).ok()?;
    let pytest = value.get("tool")?.get("pytest")?;
    if let Some(ini) = pytest.get("ini_options") {
        return toml_patterns(ini.get("python_files")?);
    }
    toml_patterns(pytest.get("python_files")?)
}

fn toml_table_python_files(text: &str, table: &str) -> Option<Vec<String>> {
    let value: toml::Value = toml::from_str(text).ok()?;
    let pytest = value.get(table)?;
    toml_patterns(pytest.get("python_files")?)
}

fn toml_patterns(value: &toml::Value) -> Option<Vec<String>> {
    let patterns = if let Some(list) = value.as_array() {
        list.iter()
            .filter_map(|item| item.as_str())
            .flat_map(split_patterns)
            .collect::<Vec<_>>()
    } else {
        split_patterns(value.as_str()?)
    };
    Some(patterns)
}

fn ini_section_present(text: &str, header: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = strip_ini_comment(line).trim();
        trimmed.eq_ignore_ascii_case(header)
    })
}

fn ini_python_files(text: &str, header: &str) -> Option<Vec<String>> {
    ini_key_patterns(text, header, "python_files")
}

fn ini_key_patterns(text: &str, header: &str, key_name: &str) -> Option<Vec<String>> {
    let mut in_section = false;
    let mut value: Option<String> = None;
    for raw in text.lines() {
        let line = strip_ini_comment(raw);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_section {
                break;
            }
            in_section = trimmed.eq_ignore_ascii_case(header);
            continue;
        }
        if !in_section {
            continue;
        }
        if raw.starts_with(char::is_whitespace) {
            if let Some(so_far) = value.as_mut() {
                if !so_far.is_empty() {
                    so_far.push(' ');
                }
                so_far.push_str(trimmed);
            }
            continue;
        }
        let Some((key, raw_value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case(key_name) {
            value = Some(raw_value.trim().to_string());
        } else if value.is_some() {
            break;
        }
    }
    value.map(|text| split_patterns(&text))
}

pub(super) fn norecursedirs_patterns_between(start: &Path, stop: &Path) -> Vec<String> {
    match pytest_config_value(start, stop, norecursedirs_from_config) {
        Some(Some(patterns)) => patterns,
        _ => default_norecursedirs(),
    }
}

fn default_norecursedirs() -> Vec<String> {
    [
        "*.egg",
        ".*",
        "_darcs",
        "build",
        "CVS",
        "dist",
        "node_modules",
        "venv",
        "{arch}",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn norecursedirs_from_config(path: &Path) -> Option<Option<Vec<String>>> {
    let name = path.file_name()?.to_str()?;
    let text = std::fs::read_to_string(path).ok()?;
    match name {
        "pytest.toml" | ".pytest.toml" => {
            Some(toml_table_key_patterns(&text, "pytest", "norecursedirs"))
        }
        "pyproject.toml" => pyproject_is_pytest_config(&text)
            .then(|| pyproject_key_patterns(&text, "norecursedirs")),
        "pytest.ini" | ".pytest.ini" => Some(ini_key_patterns(&text, "[pytest]", "norecursedirs")),
        "tox.ini" => ini_section_present(&text, "[pytest]")
            .then(|| ini_key_patterns(&text, "[pytest]", "norecursedirs")),
        "setup.cfg" => ini_section_present(&text, "[tool:pytest]")
            .then(|| ini_key_patterns(&text, "[tool:pytest]", "norecursedirs")),
        _ => None,
    }
}

fn toml_table_key_patterns(text: &str, table: &str, key: &str) -> Option<Vec<String>> {
    let value: toml::Value = toml::from_str(text).ok()?;
    toml_patterns(value.get(table)?.get(key)?)
}

fn pyproject_key_patterns(text: &str, key: &str) -> Option<Vec<String>> {
    let value: toml::Value = toml::from_str(text).ok()?;
    let pytest = value.get("tool")?.get("pytest")?;
    if let Some(ini) = pytest.get("ini_options") {
        return toml_patterns(ini.get(key)?);
    }
    toml_patterns(pytest.get(key)?)
}

pub(super) fn excluded_by_norecursedirs(
    path: &Path,
    repo_root: &Path,
    patterns: &[String],
) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let path = path.canonicalize().unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        }
    });
    let mut dir = path.parent().map(Path::to_path_buf);
    while let Some(current) = dir {
        if current != root && !current.starts_with(&root) {
            break;
        }
        if current != root && patterns.iter().any(|pattern| fnmatch_ex(pattern, &current)) {
            return true;
        }
        if current == root {
            break;
        }
        dir = current.parent().map(Path::to_path_buf);
    }
    false
}

fn strip_ini_comment(line: &str) -> &str {
    match line.find(" #") {
        Some(idx) => &line[..idx],
        None if line.trim_start().starts_with('#') => "",
        None => line,
    }
}

fn split_patterns(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(|token| token.trim_matches(['"', '\'']).to_string())
        .filter(|token| !token.is_empty())
        .collect()
}
