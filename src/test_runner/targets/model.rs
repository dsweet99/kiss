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
    #[allow(dead_code)]
    pub line_count: u32,
}

impl SourceModel {
    #[allow(dead_code)]
    pub(crate) fn all_lines(&self) -> BTreeSet<u32> {
        (1..=self.line_count).collect()
    }

    #[allow(dead_code)]
    pub(crate) fn direct_test_lines(&self) -> BTreeSet<u32> {
        let mut lines = BTreeSet::new();
        for test in &self.direct_tests {
            lines.extend(test.start_line..=test.end_line);
        }
        lines
    }

    #[allow(dead_code)]
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

/// 1-based first and last lines of a tree-sitter node; the last line is the line of
/// the node's final byte, so a node ending just after a newline ends on that line.
pub(crate) fn node_lines(node: tree_sitter::Node<'_>) -> (u32, u32) {
    let to_line = |row: usize| u32::try_from(row).unwrap_or(u32::MAX - 1).saturating_add(1);
    let start_line = to_line(node.start_position().row);
    let end = node.end_position();
    let end_line = if node.end_byte() <= node.start_byte() {
        start_line
    } else if end.column == 0 {
        to_line(end.row.saturating_sub(1))
    } else {
        to_line(end.row)
    };
    (start_line, end_line.max(start_line))
}

fn format_symbol(name: &str, member: Option<&str>) -> String {
    match member {
        Some(member) => format!("{name}.{member}"),
        None => name.to_string(),
    }
}
