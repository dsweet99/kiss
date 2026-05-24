use std::collections::HashMap;
use std::path::PathBuf;

use tree_sitter::Node;

use crate::config::Config;
use crate::violation::Violation;

use super::dependency_graph::{
    DependencyGraph, ModuleGraphMetrics, is_crate_root_aggregator, is_orphan, is_test_module,
};

include!("analyze_body.rs");
