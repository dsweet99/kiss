DEFINITION: [file] A Python or Rust source file included in analysis.
BUG: None (KPOP tried 10 hypotheses; tests in `tests/kpop_definitions.rs`: `kpop_file_definition_no_bug_found_in_10_hypotheses`)
DEFINITION: [code_unit] A named unit of code within a file (module, class/type, function, or method) that kiss can attach metrics/violations to.
BUG: None (KPOP tried 10 hypotheses; tests in `tests/kpop_definitions.rs`: `kpop_code_unit_definition_no_bug_found_in_10_hypotheses`)
DEFINITION: [statement] A statement inside a function/method body (not an import or a class/function signature).
BUG: [DONE] [DEFINITION] [statement] Outer function statement counts include nested function bodies (e.g. `outer()` counts `inner()` statements). Repro test: `bug_statement_definition_should_exclude_nested_function_bodies` in `tests/kpop_definitions.rs`.
DEFINITION: [graph_node] A module (file) in the dependency graph.
BUG: [DONE] [DEFINITION] [graph_node] Graph includes external import nodes (e.g. `os`, `json`) so graph_nodes counts non-files; definition says graph_node is a module(file). Repro test: `bug_graph_node_definition_should_exclude_external_imports` in `tests/kpop_definitions.rs`.
DEFINITION: [graph_edge] A dependency between two modules (file A depends on file B via imports/uses/mod declarations).
BUG: [DONE] [DEFINITION] [graph_edge] Dotted imports like `import pkg1.submod` can create an external node (`pkg1.submod`) instead of an internal edge to `tests.fake_python.pkg1.submod`, causing false orphan modules. Repro test: `bug_graph_edge_dotted_import_should_create_internal_edge` in `tests/kpop_definitions.rs`. Fixture: `tests/fake_python/{imports_pkg1_submod.py,imports_pkg2_submod.py,pkg1/submod.py,pkg2/submod.py}`.
RULE: [Python] [statements_per_function < 35] statements_per_function is the maximum number of statements in a Python function/method body.
BUG: [DONE] [Python] [statements_per_function] Counts statements inside nested function bodies toward the outer function. Repro test: `bug_python_function_metrics_should_not_count_nested_function_bodies` in `tests/kpop_python_function_metrics.rs`.
RULE: [Python] [positional_args < 5] positional_args is the maximum number of positional parameters in a Python function definition.
BUG: [DONE] [Python] [positional_args] `*args` is not counted as a positional parameter. Repro test: `bug_positional_args_should_count_varargs_parameter` in `tests/kpop_python_function_metrics.rs`.
RULE: [Python] [keyword_only_args < 6] keyword_only_args is the maximum number of keyword-only parameters in a Python function definition.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_keyword_only_args` in `tests/kpop_python_none.rs`)
RULE: [Python] [max_indentation_depth < 4] max_indentation_depth is the maximum indentation depth within a Python function/method body.
BUG: [DONE] [Python] [max_indentation_depth] Nested function body indentation inflates the outer function’s max indentation depth. Repro test: `bug_python_function_metrics_should_not_count_nested_function_bodies` in `tests/kpop_python_function_metrics.rs`.
RULE: [Python] [branches_per_function < 10] branches_per_function is the number of if/elif/case_clause branches in a Python function.
BUG: [DONE] [Python] [branches_per_function] Branches inside nested function bodies are counted toward the outer function. Repro test: `bug_python_function_metrics_should_not_count_nested_function_bodies` in `tests/kpop_python_function_metrics.rs`.
RULE: [Python] [local_variables_per_function < 20] local_variables_per_function is the number of distinct local variables assigned in a Python function.
BUG: [DONE] [Python] [local_variables_per_function] Local variables assigned inside nested function bodies are counted toward the outer function. Repro test: `bug_python_function_metrics_should_not_count_nested_function_bodies` in `tests/kpop_python_function_metrics.rs`.
RULE: [Python] [returns_per_function < 5] returns_per_function is the number of return statements in a Python function.
BUG: [DONE] [Python] [returns_per_function] Returns inside nested function bodies are counted toward the outer function. Repro test: `bug_python_function_metrics_should_not_count_nested_function_bodies` in `tests/kpop_python_function_metrics.rs`.
RULE: [Python] [return_values_per_function < 3] return_values_per_function is the maximum number of values returned by a single return statement in a Python function.
BUG: [DONE] [Python] [return_values_per_function] `return (a, b, c)` is counted as 1 value (should count tuple elements). Repro test: `bug_return_values_per_function_parenthesized_tuple_counts_elements` in `tests/kpop_python_function_metrics.rs`.
RULE: [Python] [nested_function_depth < 2] nested_function_depth is the maximum nesting depth of function definitions inside a Python function.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_nested_function_depth` in `tests/kpop_python_none.rs`)
RULE: [Python] [statements_per_try_block < 5] statements_per_try_block is the maximum number of statements inside any try block in a Python function.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_statements_per_try_block` in `tests/kpop_python_none.rs`)
RULE: [Python] [boolean_parameters < 1] boolean_parameters is the maximum number of boolean default parameters (True/False) in a Python function.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_boolean_parameters` in `tests/kpop_python_none.rs`)
RULE: [Python] [decorators_per_function < 3] decorators_per_function is the maximum number of decorators applied to a Python function.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_decorators_per_function` in `tests/kpop_python_none.rs`)
RULE: [Python] [calls_per_function < 50] calls_per_function is the maximum number of function/method calls in a Python function.
BUG: [DONE] [Python] [calls_per_function] Calls inside nested function bodies are counted toward the outer function. Repro test: `bug_python_function_metrics_should_not_count_nested_function_bodies` in `tests/kpop_python_function_metrics.rs`.
RULE: [Python] [methods_per_class < 20] methods_per_class is the maximum number of methods defined on a Python class.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_methods_per_class` in `tests/kpop_python_none.rs`)
RULE: [Python] [statements_per_file < 400] statements_per_file is the maximum number of statements inside function/method bodies in a Python file.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_statements_per_file` in `tests/kpop_python_none.rs`)
RULE: [Python] [functions_per_file < 30] functions_per_file is the maximum number of functions/methods defined in a Python file.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_functions_per_file` in `tests/kpop_python_none.rs`)
RULE: [Python] [interface_types_per_file < 3] interface_types_per_file is the maximum number of interface types (Protocol/ABC classes) defined in a Python file.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_interface_and_concrete_types_per_file` in `tests/kpop_python_none.rs`)
RULE: [Python] [concrete_types_per_file < 10] concrete_types_per_file is the maximum number of concrete types (non-Protocol/ABC classes) defined in a Python file.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_interface_and_concrete_types_per_file` in `tests/kpop_python_none.rs`)
RULE: [Python] [imported_names_per_file < 20] imported_names_per_file is the maximum number of unique imported names in a Python file (excluding TYPE_CHECKING-only imports).
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_imported_names_per_file` in `tests/kpop_python_none.rs`)
RULE: [Python] [cycle_size < 3] cycle_size is the maximum allowed number of modules participating in an import cycle.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_cycle_size` in `tests/kpop_python_none_graph_and_gates.rs`)
RULE: [Python] [transitive_dependencies < 100] transitive_dependencies is the maximum number of downstream modules reachable from a module in the dependency graph.
BUG: [DONE] [Python] [transitive_dependencies] External imports are counted as transitive dependencies (inflates coupling). Repro test: `bug_transitive_dependencies_should_not_count_external_modules` in `tests/kpop_python_graph_metrics.rs` (fixtures: `tests/fake_python/graph_ext_{a,b}.py`).
RULE: [Python] [dependency_depth < 7] dependency_depth is the maximum length of an import chain in the dependency graph.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_dependency_depth` in `tests/kpop_python_none_graph_and_gates.rs`)
RULE: [Python] [test_coverage_threshold >= 90] test_coverage_threshold is the minimum percent of code units whose names must appear in a test file (static check).
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_test_coverage_threshold` in `tests/kpop_python_none_graph_and_gates.rs`)
RULE: [Python] [min_similarity >= 0.70] min_similarity is the minimum similarity required to report duplicate code (when duplication_enabled=true).
BUG: None (KPOP tried 10 hypotheses; test: `kpop_python_none_min_similarity` in `tests/kpop_python_none_graph_and_gates.rs`)
RULE: [Rust] [statements_per_function < 25] statements_per_function is the maximum number of statements in a Rust function/method body.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_statements_per_function` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [arguments_per_function < 8] arguments_per_function is the maximum number of non-self parameters in a Rust function/method signature.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_arguments_per_function` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [max_indentation_depth < 4] max_indentation_depth is the maximum indentation depth within a Rust function/method body.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_max_indentation_depth_and_branches` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [branches_per_function < 8] branches_per_function is the number of `if` expressions in a Rust function.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_max_indentation_depth_and_branches` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [local_variables_per_function < 20] local_variables_per_function is the maximum number of local bindings introduced in a Rust function.
BUG: [DONE] [Rust] [local_variables_per_function] Typed patterns like `let (a, b): (i32, i32) = ...` are not counted as local variables. Repro test: `bug_rust_local_variables_should_count_typed_tuple_pattern_bindings` in `tests/kpop_rust_function_metrics.rs`.
RULE: [Rust] [returns_per_function < 5] returns_per_function is the maximum number of `return` expressions in a Rust function.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_returns_per_function` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [nested_function_depth < 2] nested_function_depth is the maximum nesting depth of closures within a Rust function.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_nested_function_depth` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [boolean_parameters < 2] boolean_parameters is the maximum number of `bool` parameters in a Rust function signature.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_boolean_parameters` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [attributes_per_function < 4] attributes_per_function is the maximum number of non-doc attributes on a Rust function.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_attributes_per_function` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [calls_per_function < 50] calls_per_function is the maximum number of function/method calls in a Rust function.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_calls_per_function` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [methods_per_class < 15] methods_per_class is the maximum number of methods in an `impl` block for a Rust type.
BUG: [DONE] [Rust] [methods_per_class] Violation metric id emitted as `methods_per_type` (does not match rule/metric name). Repro test: `bug_rust_methods_per_class_violation_metric_id_mismatch` in `tests/kpop_rust_counts_metrics.rs` (fixture: `tests/fake_rust/too_many_methods.rs`).
RULE: [Rust] [statements_per_file < 300] statements_per_file is the maximum number of statements inside function/method bodies in a Rust file.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_file_metrics_counts` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [functions_per_file < 35] functions_per_file is the maximum number of functions/methods defined in a Rust file.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_file_metrics_counts` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [interface_types_per_file < 3] interface_types_per_file is the maximum number of trait definitions in a Rust file.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_file_metrics_counts` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [concrete_types_per_file < 8] concrete_types_per_file is the maximum number of concrete type definitions (struct/enum/union) in a Rust file.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_file_metrics_counts` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [imported_names_per_file < 20] imported_names_per_file is the maximum number of internal `use` statements in a Rust file (excluding `pub use`).
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_file_metrics_counts` in `tests/kpop_rust_none.rs`)
RULE: [Rust] [cycle_size < 3] cycle_size is the maximum allowed number of modules participating in a dependency cycle.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_cycle_size` in `tests/kpop_rust_none_graph_and_gates.rs`)
RULE: [Rust] [transitive_dependencies < 50] transitive_dependencies is the maximum number of downstream modules reachable from a module in the dependency graph.
BUG: [DONE] [Rust] [transitive_dependencies] External imports (e.g. `std`) are counted as transitive dependencies. Repro test: `bug_rust_transitive_dependencies_should_not_count_external_imports` in `tests/kpop_rust_graph_metrics.rs` (fixtures: `tests/fake_rust/rust_graph_ext_{a,b}.rs`).
RULE: [Rust] [dependency_depth < 4] dependency_depth is the maximum length of a module dependency chain in the dependency graph.
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_dependency_depth` in `tests/kpop_rust_none_graph_and_gates.rs`)
RULE: [Rust] [test_coverage_threshold >= 90] test_coverage_threshold is the minimum percent of code units whose names must appear in a test file (static check).
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_test_coverage_threshold` in `tests/kpop_rust_none_graph_and_gates.rs`)
RULE: [Rust] [min_similarity >= 0.70] min_similarity is the minimum similarity required to report duplicate code (when duplication_enabled=true).
BUG: None (KPOP tried 10 hypotheses; test: `kpop_rust_none_min_similarity` in `tests/kpop_rust_none_graph_and_gates.rs`)
