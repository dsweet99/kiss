#![allow(dead_code)]

use kiss::{GateConfig, ParsedFile};

use crate::analyze::coverage_gate::evaluate_gate;
use crate::analyze::finalize::{AnalysisProducts, FinalizeAnalysisIn, finalize_analysis};
use crate::analyze::graph_api::{build_py_graph, build_rs_graph};
use crate::analyze::options::AnalyzeResult;
use crate::analyze::parallel::{BuildGraphViols, RustAnalysis, build_graph_violations};
use crate::analyze::params::GatedAnalysis;

type PyDup = kiss::DuplicateCluster;

struct GatedPyParallelIn<'a> {
    py_parsed: &'a [ParsedFile],
    opts: &'a crate::analyze::options::AnalyzeOptions<'a>,
    file_count: usize,
    gate: &'a GateConfig,
}

fn gated_py_parallel(
    in_: &GatedPyParallelIn<'_>,
) -> (
    kiss::TestRefAnalysis,
    Option<kiss::DependencyGraph>,
    Vec<kiss::Violation>,
    Vec<PyDup>,
) {
    use crate::analyze::dup_detect;

    let GatedPyParallelIn {
        py_parsed,
        opts,
        file_count,
        gate,
    } = in_;
    let orphan_enabled = gate.orphan_module_enabled;
    let dup_enabled = gate.duplication_enabled;
    let min_sim = gate.min_similarity;

    let (py_cov, (py_graph, graph_viols_all, py_dups_all)) = rayon::join(
        || {
            let py_refs: Vec<&ParsedFile> = py_parsed.iter().collect();
            kiss::analyze_test_refs_quick(&py_refs)
        },
        || {
            let py_graph = build_py_graph(py_parsed);
            let (gv, py_dups) = rayon::join(
                || {
                    build_graph_violations(BuildGraphViols {
                        py_graph: py_graph.as_ref(),
                        rs_graph: None,
                        py_config: opts.py_config,
                        rs_config: opts.rs_config,
                        file_count: *file_count,
                        orphan_enabled,
                    })
                },
                || {
                    if dup_enabled {
                        dup_detect::detect_py_duplicates(py_parsed, min_sim)
                    } else {
                        Vec::new()
                    }
                },
            );
            (py_graph, gv, py_dups)
        },
    );
    (py_cov, py_graph, graph_viols_all, py_dups_all)
}

pub(crate) fn run_gated_analysis(in_: GatedAnalysis<'_>) -> AnalyzeResult {
    use crate::analyze::dup_detect;

    let GatedAnalysis {
        opts,
        py_files,
        rs_files,
        focus_set,
        parsed: (result, viols, file_count),
        timings,
    } = in_;
    let rs_cov = kiss::analyze_rust_test_refs(&result.rs_parsed.iter().collect::<Vec<_>>(), None);

    let (py_cov, py_graph, mut graph_viols_all, py_dups_all) =
        gated_py_parallel(&GatedPyParallelIn {
            py_parsed: &result.py_parsed,
            opts,
            file_count,
            gate: opts.gate_config,
        });

    if let Some(early) = evaluate_gate(
        &py_cov,
        &rs_cov,
        focus_set,
        opts.gate_config.test_coverage_threshold,
    ) {
        return early;
    }

    let rs_graph = build_rs_graph(&result.rs_parsed);
    if let Some(ref g) = rs_graph {
        graph_viols_all.extend(kiss::analyze_graph(
            g,
            opts.rs_config,
            opts.gate_config.orphan_module_enabled,
        ));
    }
    let rs = RustAnalysis {
        graph: rs_graph,
        cov: rs_cov,
        dups: if opts.gate_config.duplication_enabled {
            dup_detect::detect_rs_duplicates(&result.rs_parsed, opts.gate_config.min_similarity)
        } else {
            Vec::new()
        },
    };

    finalize_analysis(FinalizeAnalysisIn {
        opts,
        py_files,
        rs_files,
        focus_set,
        products: AnalysisProducts {
            result,
            viols,
            file_count,
            py_cov,
            cov_viols: Vec::new(),
            coverage_cache_lists: None,
            py_stats: None,
            rs_stats: None,
            rs,
            py_graph,
            graph_viols_all,
            py_dups_all,
        },
        timings,
    })
}

#[cfg(test)]
mod gated_tests {
    use super::*;

    struct TestFixture {
        py_cfg: kiss::Config,
        rs_cfg: kiss::Config,
        gate: GateConfig,
        focus: Vec<String>,
    }

    impl TestFixture {
        fn new() -> Self {
            Self {
                py_cfg: kiss::Config::python_defaults(),
                rs_cfg: kiss::Config::rust_defaults(),
                gate: GateConfig::default(),
                focus: vec![],
            }
        }

        fn make_opts(&self) -> crate::analyze::options::AnalyzeOptions<'_> {
            crate::analyze::options::AnalyzeOptions {
                universe: "/tmp",
                focus_paths: &self.focus,
                py_config: &self.py_cfg,
                rs_config: &self.rs_cfg,
                lang_filter: None,
                bypass_gate: false,
                gate_config: &self.gate,
                ignore_prefixes: &[],
                show_timing: false,
                suppress_final_status: false,
            }
        }

        fn with_input<R>(&self, f: impl FnOnce(&GatedPyParallelIn<'_>) -> R) -> R {
            let opts = self.make_opts();
            let input = GatedPyParallelIn {
                py_parsed: &[],
                opts: &opts,
                file_count: 0,
                gate: &self.gate,
            };
            f(&input)
        }
    }

    #[test]
    fn test_gated_py_parallel_in_constructible() {
        let fix = TestFixture::new();
        fix.with_input(|input| {
            assert_eq!(input.file_count, 0);
        });
    }

    #[test]
    fn test_gated_py_parallel_empty() {
        let fix = TestFixture::new();
        fix.with_input(|input| {
            let (py_cov, py_graph, graph_viols, py_dups) = gated_py_parallel(input);
            assert!(py_cov.definitions.is_empty());
            assert!(py_graph.is_none());
            assert!(graph_viols.is_empty());
            assert!(py_dups.is_empty());
        });
    }
}
