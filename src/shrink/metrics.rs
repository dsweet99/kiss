//! Metric types for shrink constraints (split from `mod` to satisfy `concrete_types_per_file`).

use serde::{Deserialize, Serialize};

/// The five top-line metrics from the "Analyzed:" summary line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalMetrics {
    pub files: usize,
    pub code_units: usize,
    pub statements: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
}

/// Which metric is being minimized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShrinkTarget {
    Files,
    CodeUnits,
    Statements,
    GraphNodes,
    GraphEdges,
}

impl std::str::FromStr for ShrinkTarget {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "files" => Ok(Self::Files),
            "code_units" => Ok(Self::CodeUnits),
            "statements" => Ok(Self::Statements),
            "graph_nodes" => Ok(Self::GraphNodes),
            "graph_edges" => Ok(Self::GraphEdges),
            _ => Err(()),
        }
    }
}

impl ShrinkTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::CodeUnits => "code_units",
            Self::Statements => "statements",
            Self::GraphNodes => "graph_nodes",
            Self::GraphEdges => "graph_edges",
        }
    }

    pub const fn get(self, m: &GlobalMetrics) -> usize {
        match self {
            Self::Files => m.files,
            Self::CodeUnits => m.code_units,
            Self::Statements => m.statements,
            Self::GraphNodes => m.graph_nodes,
            Self::GraphEdges => m.graph_edges,
        }
    }
}
