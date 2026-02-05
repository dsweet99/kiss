# kiss

Global code feedback for LLM coding agents

## tl;dr
`kiss check` provides feedback to LLMs about code complexity, duplication, and coverage. Add an AI coder rule (e.g., a Cursor rule) like
```
When you write code, always make sure `pytest -sv tests`, `ruff check`, and `kiss check` pass.
Iterate until they do.
```
kiss will help your LLM/agent produce simpler, clearer, more maintainable code. kiss works on Python and Rust.


## The Problem: Missing Global Context
LLMs operate locally, focusing on whatever code they are editing plus bits and pieces of other, relevant code. They ignore the overall structure of the codebase because they don't see it. Over time, code tends to be a little more tangled, a little less DRY, harder to read and harder to update. To counteract this, LLMs need global information about the codebase.

kiss attempts to provide that in the form of stats about files, functions, etc., code-graph metrics, detected duplication, and low test coverage. kiss's output is compact, so it won't bloat context. It's structured for easy LLM consumption. And it's fast, so kiss can sit in your inner loop.

## Installation

```bash
cargo install kiss-ai
```

## Quickstart

In your repo:
```bash
kiss clamp
```
`kiss clamp` configures kiss to the statistics of your codebase right now. This will be the upper-bound of the complexity of your codebase. Over time, you can reduce the complexity by changing the constraints in `.kissconfig`

When your LLM runs `kiss check` it will see whether any of the code it has written has violated a constraint. For example:
```
VIOLATION:positional_args:/Users/dsweet2/Projects/kiss/tests/fake_python/deeply_nested.py:59:calculate_shipping: Function 'calculate_shipping' has 6 positional arguments (threshold: 5) Consider using keyword-only arguments, a config object, or the builder pattern.
```
Too many arguments is a [code smell](https://stackoverflow.com/questions/68069305/how-to-avoid-code-smell-too-many-parameters). kiss tells exactly where to find the problem and suggests solutions.

LLMs like to write long try/except blocks, which is terrible practice as it can hide errors and frustrate debugging.
```
VIOLATION:statements_per_try_block:/Users/dsweet2/Projects/kiss/tests/fake_python/api_handler.py:238:process_batch_operations: Function 'process_batch_operations' has 49 statements in try block (threshold: 5) Keep try blocks narrow: wrap only the code that can raise the specific exception.
```

Finally, LLMs have a tendency to rewrite small functions rather than finding and reusing them in the codebase, so kiss has a built-in (very fast!) duplicate-code detector:
```
VIOLATION:duplication:/Users/dsweet2/Projects/kiss/tests/fake_python/user_service.py:12:create_user: 80% similar, 2 copies: [/Users/dsweet2/Projects/kiss/tests/fake_python/user_service.py:12-56, /Users/dsweet2/Projects/kiss/tests/fake_python/user_service.py:104-152]. Extract common code into a shared function.
```



## Exploring your code: `kiss stats` and `kiss viz`

At any time you can run
```bash
kiss stats
```
to see the distribution of metrics for your codebase. For example

```
$ kiss stats
kiss stats - Summary Statistics
Analyzed from: .

=== Rust (36 files) ===
Metric                            Count    50%    90%    95%    99%    Max
--------------------------------------------------------------------------
Statements per function             562      4     12     16     21     25
Arguments (total)                   562      1      3      4      6      7
Arguments (positional)              562      1      3      4      6      7
Arguments (keyword-only)            562      0      0      0      0      0
Max indentation depth               562      0      2      2      4      4
Nested function depth               562      0      1      1      2      2
Returns per function                562      0      0      1      2      4
Branches per function               562      0      2      2      4      7
Local variables per function        562      1      5      5      8     12
Methods per class                    31      1      7      9     14     14
Statements per file                  36     56    116    134    148    148
Classes per file                     36      1      4      4      5      5
Imported names per file              36      3      8      8     20     20
Fan-in (per module)                  44      1      3      9     26     26
Fan-out (per module)                 44      2      3      3     24     24
Transitive deps (per module)         44      2      3      3     29     29
Dependency depth (per module)        44      1      1      1      2      2
```

If you notice some outliers, try editing `.kissconfig` then asking your LLM to make `kiss check` pass. Watch it refactor and simplify your codebase.

To visualize your code graph, try
```bash
kiss viz graph.md --zoom=0.25
```
This will create a Mermaid plot inside the markdown file graph.md (viewable in VSCode/Cursor). The argument `--zoom=0.25` zooms out on the code graph, coalescing nodes, giving a simplified view. This can be helpful for larger codebases. `--zoom=1.0` includes every code unit. `--zoom=0.0` produces the trivial graph with one node representing your entire codebase.

![Dependency graph example](images/graph.png)


## kiss rules

You can help your LLM produce rule-following code by adding the output of `kiss rules` (see below) to its context before it starts coding. For example, you might put this in `.cursorrules` (or maybe `AGENTS.md`):
```
FIRST STEP: After the user's first request, before doing anything else, call `kiss rules`
```


The rules that `kiss rules` dumps to stdout are the same rules that kiss will enforce when you run `kiss check`. Note that the threshold numbers in the output you see will come from your actual kiss config.

```
$ kiss rules
DEFINITION: [file] A Python or Rust source file included in analysis.
DEFINITION: [code_unit] A named unit of code within a file (module, class/type, function, or method) that kiss can attach metrics/violations to.
DEFINITION: [statement] A statement inside a function/method body (not an import or a class/function signature).
DEFINITION: [graph_node] A module (file) in the dependency graph.
DEFINITION: [graph_edge] A dependency between two modules (file A depends on file B via imports/uses/mod declarations).
RULE: [Python] [statements_per_function < 35] statements_per_function is the maximum number of statements in a Python function/method body.
RULE: [Python] [positional_args < 5] positional_args is the maximum number of positional parameters in a Python function definition.
RULE: [Python] [keyword_only_args < 6] keyword_only_args is the maximum number of keyword-only parameters in a Python function definition.
RULE: [Python] [max_indentation_depth < 4] max_indentation_depth is the maximum indentation depth within a Python function/method body.
RULE: [Python] [branches_per_function < 10] branches_per_function is the number of if/elif/case_clause branches in a Python function.
RULE: [Python] [local_variables_per_function < 20] local_variables_per_function is the number of distinct local variables assigned in a Python function.
RULE: [Python] [returns_per_function < 5] returns_per_function is the number of return statements in a Python function.
RULE: [Python] [return_values_per_function < 3] return_values_per_function is the maximum number of values returned by a single return statement in a Python function.
RULE: [Python] [nested_function_depth < 2] nested_function_depth is the maximum nesting depth of function definitions inside a Python function.
RULE: [Python] [statements_per_try_block < 5] statements_per_try_block is the maximum number of statements inside any try block in a Python function.
RULE: [Python] [boolean_parameters < 1] boolean_parameters is the maximum number of boolean default parameters (True/False) in a Python function.
RULE: [Python] [decorators_per_function < 3] decorators_per_function is the maximum number of decorators applied to a Python function.
RULE: [Python] [calls_per_function < 50] calls_per_function is the maximum number of function/method calls in a Python function.
RULE: [Python] [methods_per_class < 20] methods_per_class is the maximum number of methods defined on a Python class.
RULE: [Python] [statements_per_file < 400] statements_per_file is the maximum number of statements inside function/method bodies in a Python file.
RULE: [Python] [functions_per_file < 30] functions_per_file is the maximum number of functions/methods defined in a Python file.
RULE: [Python] [interface_types_per_file < 3] interface_types_per_file is the maximum number of interface types (Protocol/ABC classes) defined in a Python file.
RULE: [Python] [concrete_types_per_file < 10] concrete_types_per_file is the maximum number of concrete types (non-Protocol/ABC classes) defined in a Python file.
RULE: [Python] [imported_names_per_file < 20] imported_names_per_file is the maximum number of unique imported names in a Python file (excluding TYPE_CHECKING-only imports).
RULE: [Python] [cycle_size < 3] cycle_size is the maximum allowed number of modules participating in an import cycle.
RULE: [Python] [transitive_dependencies < 100] transitive_dependencies is the maximum number of downstream modules reachable from a module in the dependency graph.
RULE: [Python] [dependency_depth < 7] dependency_depth is the maximum length of an import chain in the dependency graph.
RULE: [Python] [test_coverage_threshold >= 90] test_coverage_threshold is the minimum percent of code units whose names must appear in a test file (static check).
RULE: [Python] [min_similarity >= 0.70] min_similarity is the minimum similarity required to report duplicate code (when duplication_enabled=true).
RULE: [Rust] [statements_per_function < 25] statements_per_function is the maximum number of statements in a Rust function/method body.
RULE: [Rust] [arguments_per_function < 8] arguments_per_function is the maximum number of non-self parameters in a Rust function/method signature.
RULE: [Rust] [max_indentation_depth < 4] max_indentation_depth is the maximum indentation depth within a Rust function/method body.
RULE: [Rust] [branches_per_function < 8] branches_per_function is the number of `if` expressions in a Rust function.
RULE: [Rust] [local_variables_per_function < 20] local_variables_per_function is the maximum number of local bindings introduced in a Rust function.
RULE: [Rust] [returns_per_function < 5] returns_per_function is the maximum number of `return` expressions in a Rust function.
RULE: [Rust] [nested_function_depth < 2] nested_function_depth is the maximum nesting depth of closures within a Rust function.
RULE: [Rust] [boolean_parameters < 2] boolean_parameters is the maximum number of `bool` parameters in a Rust function signature.
RULE: [Rust] [attributes_per_function < 4] attributes_per_function is the maximum number of non-doc attributes on a Rust function.
RULE: [Rust] [calls_per_function < 50] calls_per_function is the maximum number of function/method calls in a Rust function.
RULE: [Rust] [methods_per_class < 15] methods_per_class is the maximum number of methods in an `impl` block for a Rust type.
RULE: [Rust] [statements_per_file < 300] statements_per_file is the maximum number of statements inside function/method bodies in a Rust file.
RULE: [Rust] [functions_per_file < 35] functions_per_file is the maximum number of functions/methods defined in a Rust file.
RULE: [Rust] [interface_types_per_file < 3] interface_types_per_file is the maximum number of trait definitions in a Rust file.
RULE: [Rust] [concrete_types_per_file < 8] concrete_types_per_file is the maximum number of concrete type definitions (struct/enum/union) in a Rust file.
RULE: [Rust] [imported_names_per_file < 20] imported_names_per_file is the maximum number of internal `use` statements in a Rust file (excluding `pub use`).
RULE: [Rust] [cycle_size < 3] cycle_size is the maximum allowed number of modules participating in a dependency cycle.
RULE: [Rust] [transitive_dependencies < 50] transitive_dependencies is the maximum number of downstream modules reachable from a module in the dependency graph.
RULE: [Rust] [dependency_depth < 4] dependency_depth is the maximum length of a module dependency chain in the dependency graph.
RULE: [Rust] [test_coverage_threshold >= 90] test_coverage_threshold is the minimum percent of code units whose names must appear in a test file (static check).
RULE: [Rust] [min_similarity >= 0.70] min_similarity is the minimum similarity required to report duplicate code (when duplication_enabled=true).
```


## kiss shrink: How simple is simple enough?

When you run `kiss check`, you'll see some global metrics like
```
Analyzed: 45 files, 991 code_units, 3323 statements, 57 graph_nodes, 129 graph_edges
```
kiss doesn't normally constrain these because, if it did, you wouldn't be able to add new features. But for a fixed set of functionality, you probably want the simplest codebase you can have.  kiss helps with the `shrink` workflow. Initialize with
```
kiss shrink graph_edges=120
```
This tells kiss to clamp all the global metrics to their current values *except* for the one you specify. In this case, `graph_edges`, which kiss clamps to 120, slightly lower than the current 129. You may choose any number as the constraint.

Next, ask your LLM to iterate until
```
kiss shrink
```
passes.  `kiss shrink` will flag `graph_edges>120` as a violation, trigger your LLM to refactor, reducing the number of connections between different code units without increasing any other global measure or any of the usual `kiss check` measures. This would tend to make your code more cohesive.

Choosing to constrain different global metrics will have different effects:

| Target | What the LLM would do |
|---|---|
| statements | Remove dead code, simplify conditionals, inline trivial functions |
| code_units | Consolidate similar functions, remove unused helpers |
| graph_edges | Remove unnecessary imports, colocate tightly-coupled code |
| graph_nodes | Merge small modules, remove orphan modules |

Note that only `kiss shrink` will constrain global metrics. `kiss check` will ignore them.
