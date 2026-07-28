//! Source-model spans for explicit test-target selection.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use kiss::Language;

use super::{model_python, model_rust};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DirectTestDef {
    pub selector: String,
    pub name: String,
    pub owner: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NamedDefinition {
    pub name: String,
    pub member: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub is_unit_test: bool,
    pub test_selector: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceModel {
    pub path: PathBuf,
    pub language: Language,
    pub direct_tests: Vec<DirectTestDef>,
    pub definitions: Vec<NamedDefinition>,
    pub line_count: u32,
}

impl SourceModel {
    pub(crate) fn all_lines(&self) -> BTreeSet<u32> {
        (1..=self.line_count).collect()
    }

    pub(crate) fn direct_test_lines(&self) -> BTreeSet<u32> {
        let mut lines = BTreeSet::new();
        for test in &self.direct_tests {
            lines.extend(test.start_line..=test.end_line);
        }
        lines
    }

    pub(crate) fn non_test_lines(&self) -> BTreeSet<u32> {
        let test_lines = self.direct_test_lines();
        self.all_lines()
            .into_iter()
            .filter(|line| !test_lines.contains(line))
            .collect()
    }

    pub(crate) fn find_definition(
        &self,
        name: &str,
        member: Option<&str>,
    ) -> Result<&NamedDefinition, String> {
        let matches: Vec<_> = self
            .definitions
            .iter()
            .filter(|def| def.name == name && def.member.as_deref() == member)
            .collect();
        match matches.as_slice() {
            [one] => Ok(*one),
            [] => Err(format!(
                "unresolved symbol '{}' in {}",
                format_symbol(name, member),
                self.path.display()
            )),
            _ => Err(format!(
                "ambiguous symbol '{}' in {}",
                format_symbol(name, member),
                self.path.display()
            )),
        }
    }

    pub(crate) fn coverage_lines_for_definition(&self, def: &NamedDefinition) -> BTreeSet<u32> {
        let mut lines: BTreeSet<u32> = (def.start_line..=def.end_line).collect();
        for test in &self.direct_tests {
            if test.start_line >= def.start_line && test.end_line <= def.end_line {
                for line in test.start_line..=test.end_line {
                    lines.remove(&line);
                }
            }
        }
        lines
    }
}

pub(crate) fn load_source_model(path: &Path, language: Language) -> Result<SourceModel, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let line_count = u32::try_from(content.lines().count()).unwrap_or(u32::MAX);
    match language {
        Language::Python => model_python::build_python_model(path, content, line_count),
        Language::Rust => model_rust::build_rust_model(path, content, line_count),
    }
}

pub(crate) fn byte_span_to_lines(content: &str, start: usize, end: usize) -> (u32, u32) {
    let end = end.max(start);
    let start_line = line_number_at(content, start);
    let end_line = if end == 0 {
        start_line
    } else {
        line_number_at(content, end.saturating_sub(1))
    };
    (start_line, end_line.max(start_line))
}

fn line_number_at(content: &str, byte_offset: usize) -> u32 {
    let offset = byte_offset.min(content.len());
    let mut line = 1u32;
    for (idx, ch) in content.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line = line.saturating_add(1);
        }
    }
    line
}

fn format_symbol(name: &str, member: Option<&str>) -> String {
    match member {
        Some(member) => format!("{name}.{member}"),
        None => name.to_string(),
    }
}
