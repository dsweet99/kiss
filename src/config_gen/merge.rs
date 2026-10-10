use std::path::Path;

pub fn format_section(out: &mut String, name: &str, section: Option<&toml::Value>) {
    use std::fmt::Write;
    if let Some(v) = section {
        let _ = writeln!(out, "[{name}]");
        if let Some(t) = v.as_table() {
            for (k, v) in t {
                let _ = writeln!(out, "{k} = {v}");
            }
        }
        out.push('\n');
    }
}

#[derive(Clone, Copy)]
pub enum MergeLanguageUpdate {
    PythonOnly,
    RustOnly,
    Both,
    Neither,
}

impl MergeLanguageUpdate {
    pub const fn from_analyzed_counts(py_cnt: usize, rs_cnt: usize) -> Self {
        match (py_cnt > 0, rs_cnt > 0) {
            (true, true) => Self::Both,
            (true, false) => Self::PythonOnly,
            (false, true) => Self::RustOnly,
            (false, false) => Self::Neither,
        }
    }

    const fn update_python(self) -> bool {
        matches!(self, Self::PythonOnly | Self::Both)
    }

    const fn update_rust(self) -> bool {
        matches!(self, Self::RustOnly | Self::Both)
    }

    const fn update_both(self) -> bool {
        matches!(self, Self::Both)
    }
}

pub fn merge_config_toml(path: &Path, new: &str, lang: MergeLanguageUpdate) -> String {
    let Some((ex, nw)) = load_merge_tables(path, new) else {
        return new.to_string();
    };
    let mut merged = toml::Table::new();
    merge_global(&mut merged, &ex, &nw);
    merge_lang_sections(&mut merged, &ex, &nw, lang);
    merge_test(&mut merged, &ex, &nw);
    merge_shared(&mut merged, &ex, &nw, lang);
    merge_thresholds(&mut merged, &ex, lang);
    relocate_language_job_keys(&mut merged, &ex, &nw);
    build_merged_output(&merged)
}

pub(super) fn load_merge_tables(path: &Path, new: &str) -> Option<(toml::Table, toml::Table)> {
    let ex_str = std::fs::read_to_string(path).ok()?;
    let ex = ex_str.parse::<toml::Table>().ok()?;
    let nw = new.parse::<toml::Table>().ok()?;
    Some((ex, nw))
}

pub(super) fn merge_global(merged: &mut toml::Table, ex: &toml::Table, nw: &toml::Table) {
    if let Some(v) = ex
        .get("global")
        .cloned()
        .or_else(|| nw.get("global").cloned())
    {
        merged.insert("global".to_string(), v);
    }
}

pub(super) fn merge_lang_sections(
    merged: &mut toml::Table,
    ex: &toml::Table,
    nw: &toml::Table,
    lang: MergeLanguageUpdate,
) {
    for (k, upd) in [
        ("python", lang.update_python()),
        ("rust", lang.update_rust()),
    ] {
        let chosen = if upd {
            nw.get(k)
                .cloned()
                .or_else(|| ex.get(k).cloned())
                .map(|value| keep_existing_max_num_tests(value, ex.get(k)))
        } else {
            ex.get(k).cloned().or_else(|| nw.get(k).cloned())
        };
        let value = chosen.unwrap_or_else(|| default_language_table(k));
        merged.insert(k.to_string(), value);
    }
}

fn keep_existing_max_num_tests(chosen: toml::Value, previous: Option<&toml::Value>) -> toml::Value {
    let Some(prev_cap) = previous
        .and_then(|value| value.as_table())
        .and_then(|table| table.get("max_num_tests"))
        .cloned()
    else {
        return chosen;
    };
    let toml::Value::Table(mut table) = chosen else {
        return chosen;
    };
    table.insert("max_num_tests".to_string(), prev_cap);
    toml::Value::Table(table)
}

fn default_language_table(name: &str) -> toml::Value {
    let mut text = String::new();
    match name {
        "python" => super::defaults_append::append_python_defaults(&mut text),
        "rust" => super::defaults_append::append_rust_defaults(&mut text),
        _ => return toml::Value::Table(toml::Table::new()),
    }
    text.parse::<toml::Table>()
        .expect("default language section is valid toml")
        .get(name)
        .cloned()
        .expect("default language section has its table")
}

const TEST_GATE_MERGE_KEYS: &[&str] = &["orphan_detection", "max_unit_test_seconds"];

pub(super) fn merge_test(merged: &mut toml::Table, ex: &toml::Table, nw: &toml::Table) {
    let mut table = toml::Table::new();
    if let Some(toml::Value::Table(ex_t)) = ex.get("test") {
        for (k, v) in ex_t {
            table.insert(k.clone(), v.clone());
        }
    }
    if let Some(toml::Value::Table(nw_t)) = nw.get("test") {
        if table.is_empty() {
            for (k, v) in nw_t {
                table.insert(k.clone(), v.clone());
            }
        } else {
            for key in TEST_GATE_MERGE_KEYS {
                if table.contains_key(*key) {
                    continue;
                }
                if let Some(v) = nw_t.get(*key) {
                    table.insert((*key).to_string(), v.clone());
                }
            }
        }
    }
    table.remove("max_num_tests");
    for key in ["num_jobs_pytest", "num_jobs_nextest", "num_jobs_llvm_cov"] {
        table.remove(key);
    }
    if !table.is_empty() {
        merged.insert("test".to_string(), toml::Value::Table(table));
    }
}

pub(super) fn merge_shared(
    merged: &mut toml::Table,
    ex: &toml::Table,
    nw: &toml::Table,
    lang: MergeLanguageUpdate,
) {
    let shared = if lang.update_both() {
        nw.get("shared")
    } else {
        ex.get("shared").or_else(|| nw.get("shared"))
    }
    .cloned();
    if let Some(v) = shared {
        merged.insert("shared".to_string(), v);
    }
}

pub(super) fn merge_thresholds(
    merged: &mut toml::Table,
    ex: &toml::Table,
    lang: MergeLanguageUpdate,
) {
    if !lang.update_both()
        && let Some(v) = ex.get("thresholds").cloned()
    {
        merged.insert("thresholds".to_string(), v);
    }
}

fn relocate_language_job_keys(merged: &mut toml::Table, ex: &toml::Table, nw: &toml::Table) {
    if let Some(value) = preferred_pytest_jobs(ex, nw) {
        upsert_language_key(merged, "python", "num_jobs_pytest", value);
    }
    if let Some(value) = preferred_nextest_jobs(ex, nw) {
        upsert_language_key(merged, "rust", "num_jobs_nextest", value);
        drop_rust_job_alias(merged);
    }
}

fn preferred_nextest_jobs(ex: &toml::Table, nw: &toml::Table) -> Option<toml::Value> {
    const KEYS: [&str; 2] = ["num_jobs_nextest", "num_jobs_llvm_cov"];
    section_values(ex, "rust", &KEYS)
        .or_else(|| section_values(ex, "test", &KEYS))
        .or_else(|| section_values(nw, "rust", &KEYS))
        .or_else(|| section_values(nw, "test", &KEYS))
}

fn preferred_pytest_jobs(ex: &toml::Table, nw: &toml::Table) -> Option<toml::Value> {
    section_value(ex, "python", "num_jobs_pytest")
        .or_else(|| section_value(ex, "test", "num_jobs_pytest"))
        .or_else(|| section_value(nw, "python", "num_jobs_pytest"))
        .or_else(|| section_value(nw, "test", "num_jobs_pytest"))
}

fn section_values(root: &toml::Table, section: &str, keys: &[&str]) -> Option<toml::Value> {
    keys.iter()
        .find_map(|key| section_value(root, section, key))
}

fn section_value(root: &toml::Table, section: &str, key: &str) -> Option<toml::Value> {
    root.get(section)
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get(key))
        .cloned()
}

fn upsert_language_key(merged: &mut toml::Table, section: &str, key: &str, value: toml::Value) {
    let Some(toml::Value::Table(table)) = merged.get_mut(section) else {
        return;
    };
    table.insert(key.to_string(), value);
}

fn drop_rust_job_alias(merged: &mut toml::Table) {
    let Some(toml::Value::Table(table)) = merged.get_mut("rust") else {
        return;
    };
    if table.contains_key("num_jobs_nextest") {
        table.remove("num_jobs_llvm_cov");
    }
}

pub fn build_merged_output(m: &toml::Table) -> String {
    let mut out = String::from(
        "# Generated by kiss check\n# Thresholds based on max values of analyzed codebase\n\n",
    );
    for k in ["global", "python", "rust", "test", "shared", "thresholds"] {
        format_section(&mut out, k, m.get(k));
    }
    out
}
