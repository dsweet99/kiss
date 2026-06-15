use kiss::discovery::gather_files_by_lang;
use kiss::{Config, Language};

use crate::analyze::graph_api::{build_graphs, graph_stats};
use crate::analyze_parse::parse_all;

/// Inputs for [`compute_global_metrics`].
pub struct GlobalMetricsInput<'a> {
    pub paths: &'a [String],
    pub ignore: &'a [String],
    pub lang_filter: Option<Language>,
    pub py_config: &'a Config,
    pub rs_config: &'a Config,
}

pub fn compute_global_metrics(in_: &GlobalMetricsInput<'_>) -> Option<kiss::GlobalMetrics> {
    let GlobalMetricsInput {
        paths,
        ignore,
        lang_filter,
        py_config,
        rs_config,
    } = in_;
    let (py_files, rs_files) = gather_files_by_lang(paths, *lang_filter, ignore);
    if py_files.is_empty() && rs_files.is_empty() {
        return None;
    }
    let result = parse_all(&py_files, &rs_files, py_config, rs_config);
    let (py_graph, rs_graph) = build_graphs(&result.py_parsed, &result.rs_parsed);
    let (nodes, edges) = graph_stats(py_graph.as_ref(), rs_graph.as_ref());
    Some(kiss::GlobalMetrics {
        files: result.py_parsed.len() + result.rs_parsed.len(),
        code_units: result.code_unit_count,
        statements: result.statement_count,
        graph_nodes: nodes,
        graph_edges: edges,
    })
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl GlobalMetricsInput<'_> {
        fn witness() {}
    }

    #[test]
    fn witness_global_metrics_input() {
        GlobalMetricsInput::witness();
        let py = Config::python_defaults();
        let rs = Config::rust_defaults();
        let input = GlobalMetricsInput {
            paths: &[],
            ignore: &[],
            lang_filter: None,
            py_config: &py,
            rs_config: &rs,
        };
        assert!(compute_global_metrics(&input).is_none());
    }
}
