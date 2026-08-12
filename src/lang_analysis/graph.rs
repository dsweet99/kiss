//! Build dependency graphs for one supported language.

use super::analysis::{LanguageAnalysis, PythonAnalysis, RustAnalysis};
use crate::graph::{DependencyGraph, build_dependency_graph};
use crate::parsing::ParsedFile;
use crate::rust_graph::build_rust_dependency_graph;
use crate::rust_parsing::ParsedRustFile;

/// Build a dependency graph for one supported language.
pub trait LanguageGraph: LanguageAnalysis {
    type Parsed;
    fn build_graph(&self, parsed: &[&Self::Parsed]) -> DependencyGraph;
}

impl LanguageGraph for PythonAnalysis {
    type Parsed = ParsedFile;

    fn build_graph(&self, parsed: &[&Self::Parsed]) -> DependencyGraph {
        build_dependency_graph(parsed)
    }
}

impl LanguageGraph for RustAnalysis {
    type Parsed = ParsedRustFile;

    fn build_graph(&self, parsed: &[&Self::Parsed]) -> DependencyGraph {
        build_rust_dependency_graph(parsed)
    }
}

/// Build graphs for both languages through the abstract graph trait.
pub fn build_graphs(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
) -> (Option<DependencyGraph>, Option<DependencyGraph>) {
    let py = if py_parsed.is_empty() {
        None
    } else {
        let refs: Vec<_> = py_parsed.iter().collect();
        Some(PythonAnalysis.build_graph(&refs))
    };
    let rs = if rs_parsed.is_empty() {
        None
    } else {
        let refs: Vec<_> = rs_parsed.iter().collect();
        Some(RustAnalysis.build_graph(&refs))
    };
    (py, rs)
}
