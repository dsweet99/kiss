//! Python dependency graph construction and analysis.
//!
//! Bodies are `include!`d into this single module so the Rust module graph does not add nested
//! `graph::*` submodules (keeps `indirect_dependencies` metrics low).

use crate::config::Config;
use crate::parsing::ParsedFile;
use crate::violation::Violation;
use petgraph::Direction;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use rayon::prelude::*;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use tree_sitter::Node;

include!("dependency_graph_body.rs");
include!("python_imports_body.rs");
include!("build_body.rs");
include!("analyze_body.rs");

#[cfg(test)]
#[path = "graph_test.rs"]
mod tests;

#[cfg(test)]
#[path = "graph_test_2.rs"]
mod graph_test_2;
