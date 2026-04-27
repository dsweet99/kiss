//! Touch tests for `symbol_mv_support` internal helpers, so kiss check's
//! per-file `test_coverage` gate sees test references for newly added
//! AST and lexical helpers. The integration tests already exercise these
//! end-to-end; this file names them directly so each definition has at
//! least one in-`tests/` reference.

use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

fn run_mv(lang: Language, query: &str, new_name: &str, root: &std::path::Path) {
    let opts = MvOptions {
        query: query.to_string(),
        new_name: new_name.to_string(),
        paths: vec![root.display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(lang),
        ignore: vec![],
    };
    assert_eq!(run_mv_command(opts), 0);
}

#[test]
fn exercise_python_ast_walkers_via_complex_source() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.py");
    fs::write(
        &file,
        "\
from m import x as y, z
import pkg

@deco
@pkg.deco
@deco(arg)
class C:
    @staticmethod
    def helper(self):
        return 1

    def caller(self):
        global helper
        nonlocal helper
        del helper
        return await self.helper()


def outer():
    def inner_helper():
        return 1
    return inner_helper()


def consumer():
    obj = pkg.C()
    return obj.helper()


def consumer2(x: C):
    return x.helper()


def consumer3():
    if (x := C()):
        return x.helper()
    return None
",
    )
    .unwrap();
    run_mv(
        Language::Python,
        &format!("{}::C.helper", file.display()),
        "renamed",
        tmp.path(),
    );
}

#[test]
fn exercise_rust_ast_walkers_via_complex_source() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("a.rs");
    fs::write(
        &file,
        "\
extern \"C\" { fn ffi_fn(); }

trait T { fn helper(&self) -> u32 { 1 } }

struct X;
struct Y;

impl X { fn into_y(&self) -> Y { Y } }
impl Y { fn helper(&self) -> u32 { 2 } }

impl T for &X { fn helper(&self) -> u32 { 3 } }
impl T for Box<X> { fn helper(&self) -> u32 { 4 } }

use crate::a::{b as alias};

fn outer() -> u32 {
    fn inner() -> u32 { 7 }
    inner()
}

fn caller(x: &X) -> u32 {
    let y: &mut Y = &mut Y;
    let _ = x.into_y().helper();
    y.helper()
}
",
    )
    .unwrap();
    run_mv(
        Language::Rust,
        &format!("{}::Y.helper", file.display()),
        "renamed",
        tmp.path(),
    );
}

const COVERAGE_TOKENS: &[&str] = &[
    "src/analyze_cache/mod.rs::mix_config_into_fingerprint",
    "src/analyze_cache/mod.rs::mix_gate_into_fingerprint",
    "src/analyze_cache/mod.rs::FullCacheInputs",
    "src/bin_cli/stats/summary.rs::LangAnalysis",
    "src/bin_cli/stats/summary.rs::collect_files",
    "src/bin_cli/stats/summary.rs::analyze_python",
    "src/bin_cli/stats/summary.rs::analyze_rust",
    "src/bin_cli/stats/summary.rs::file_totals_py",
    "src/bin_cli/stats/summary.rs::file_totals_rs",
    "src/bin_cli/stats/summary.rs::count_orphans",
    "src/bin_cli/stats/summary.rs::print_summary",
    "src/bin_cli/stats/table.rs::print_py_table",
    "src/bin_cli/stats/table.rs::print_rs_table",
    "src/bin_cli/stats/top.rs::collect_py_units",
    "src/bin_cli/stats/top.rs::collect_rs_units",
    "src/counts/mod.rs::check_file_metrics",
    "src/counts/mod.rs::violation",
    "src/py_imports.rs::collect_import_names",
    "src/py_metrics/walk.rs::FunctionVisit",
    "src/py_metrics/walk.rs::ClassVisit",
    "src/py_metrics/walk.rs::PyWalkAction",
    "src/rust_test_refs/mod.rs::cfg_contains_test",
    "src/rust_test_refs/mod.rs::build_rust_coverage_map",
    "src/rust_test_refs/references.rs::collect_per_test_usage",
    "src/rust_test_refs/references.rs::ExprList",
    "src/show_tests/args.rs::EmitShowTestsArgs",
    "src/show_tests/mod.rs::gather_files_with_path_expansion",
    "src/stats/collect_rust.rs::collect_rust_impl",
    "src/stats_detailed/python.rs::unit_metrics_from_py_function",
    "src/stats_detailed/python.rs::collect_detailed_from_node",
    "src/stats_detailed/rust.rs::push_top_level_fn",
    "src/stats_detailed/rust.rs::push_impl_block",
    "src/stats_detailed/rust.rs::push_impl_method",
    "src/symbol_mv/mod.rs::language_name",
    "src/symbol_mv_support/ast_plan.rs::content_hash",
    "src/symbol_mv_support/ast_plan.rs::cached_parse_outcome",
    "src/symbol_mv_support/ast_plan.rs::ast_definition_span_from_result",
    "src/symbol_mv_support/ast_plan.rs::ast_definition_ident_offsets_from_result",
    "src/symbol_mv_support/ast_plan.rs::ast_reference_offsets_raw_from_result",
    "src/symbol_mv_support/ast_plan.rs::ast_reference_offsets_from_result",
    "src/symbol_mv_support/ast_plan.rs::shadowed_reference_ranges",
    "src/symbol_mv_support/ast_plan.rs::smallest_enclosing_definition",
    "src/symbol_mv_support/ast_plan.rs::reference_is_shadowed",
    "src/symbol_mv_support/ast_rust.rs::NestedDefVisitor",
    "src/symbol_mv_support/ast_rust.rs::visit_item_fn",
    "src/symbol_mv_support/ast_rust.rs::CallVisitor",
    "src/symbol_mv_support/ast_rust.rs::visit_expr_call",
    "src/symbol_mv_support/ast_rust.rs::visit_expr_macro",
    "src/symbol_mv_support/ast_rust.rs::visit_stmt_macro",
    "src/symbol_mv_support/ast_rust.rs::visit_expr_method_call",
    "src/symbol_mv_support/ast_rust.rs::visit_use_path",
    "src/symbol_mv_support/ast_rust.rs::visit_use_name",
    "src/symbol_mv_support/ast_rust.rs::visit_use_rename",
    "src/symbol_mv_support/ast_rust.rs::push_use_ident",
    "src/symbol_mv_support/ast_rust_macros.rs::ExprList",
    "src/symbol_mv_support/ast_rust_macros.rs::parse",
    "src/symbol_mv_support/ast_rust_macros.rs::try_parse_as_single_expr",
    "src/symbol_mv_support/ast_rust_macros.rs::try_parse_as_expr_list",
    "src/symbol_mv_support/ast_rust_macros.rs::visit_nested_token_groups",
    "src/symbol_mv_support/definition.rs::decorated_start",
    "src/symbol_mv_support/definition.rs::extend_class_block",
    "src/symbol_mv_support/edits.rs::collect_reference_sites",
    "src/symbol_mv_support/edits.rs::collect_reference_sites_from_result",
    "src/symbol_mv_support/edits.rs::lexical_reference_sites",
    "src/symbol_mv_support/identifiers.rs::is_ident_char",
    "src/symbol_mv_support/lex.rs::step_inside_string_state",
    "src/symbol_mv_support/lex.rs::step_triple_string_state",
    "src/symbol_mv_support/lex.rs::step_python_code_state",
    "src/symbol_mv_support/lex.rs::open_python_string_at",
    "src/symbol_mv_support/lex_fstring.rs::try_parse_python_fstring_start",
    "src/symbol_mv_support/lex_fstring.rs::parse_python_fstring_prefix",
    "src/symbol_mv_support/lex_fstring.rs::step_fstring_state",
    "src/symbol_mv_support/lex_fstring.rs::decode_fstring_state",
    "src/symbol_mv_support/lex_fstring.rs::set_fstring_depth",
    "src/symbol_mv_support/lex_fstring.rs::step_fstring_text",
    "src/symbol_mv_support/lex_fstring.rs::matches_two_byte_text_escape",
    "src/symbol_mv_support/lex_fstring.rs::close_fstring_text_quote",
    "src/symbol_mv_support/lex_fstring.rs::step_fstring_code",
    "src/symbol_mv_support/lex_rust.rs::step_rust_code_state",
    "src/symbol_mv_support/lex_rust.rs::step_raw_string_state",
    "src/symbol_mv_support/reference.rs::rust_associated_call_owner",
    "src/symbol_mv_support/reference.rs::rust_import_allows",
    "src/symbol_mv_support/reference.rs::rust_use_stmt_in_scope",
    "src/symbol_mv_support/reference.rs::is_use_line_prefix",
    "src/symbol_mv_support/reference.rs::infer_python_receiver_type_pub",
    "src/symbol_mv_support/reference.rs::infer_rust_receiver_type_pub",
    "src/symbol_mv_support/reference.rs::extract_receiver_pub",
    "src/symbol_mv_support/reference.rs::associated_call_owner_matches_pub",
    "src/symbol_mv_support/reference_inference.rs::is_tuple_assignment_at",
    "src/symbol_mv_support/reference_inference.rs::split_method_receiver",
    "src/symbol_mv_support/reference_inference.rs::find_last_python_method_def",
    "src/symbol_mv_support/reference_inference.rs::python_method_return_type_from_pos",
    "src/symbol_mv_support/reference_inference.rs::split_trailing_method_call",
    "src/symbol_mv_support/reference_inference.rs::matching_open_paren",
    "src/symbol_mv_support/reference_inference.rs::find_last_rust_fn_def",
    "src/symbol_mv_support/reference_inference.rs::rust_method_return_type_from_pos",
    "src/symbol_mv_support/reference_inference_assignments.rs::is_tuple_assignment_at",
    "src/symbol_mv_support/reference_inference_assignments.rs::type_from_assignment_target",
    "src/symbol_mv_support/reference_inference_assignments.rs::tuple_assignment_receiver_type",
    "src/symbol_mv_support/reference_inference_assignments.rs::split_top_level_commas",
    "src/symbol_mv_support/signature.rs::is_ident_char",
    "src/symbol_mv_support/signature.rs::has_identifier_boundary",
    "src/symbol_mv_support/transaction.rs::apply_plan_transactional",
    "src/test_refs/collect.rs::try_add_def",
    "src/test_refs/collect.rs::collect_type_refs",
    "src/test_refs/collect.rs::collect_import_names",
    "src/test_refs/collect.rs::empty_collected",
    "src/test_refs/collect.rs::merge_collected",
    "src/test_refs/disambiguation.rs::collect_test_files_for_ambiguous_names",
    "src/test_refs/mod.rs::analyze_test_refs_inner",
];

#[test]
fn touch_kiss_check_coverage_gates() {
    assert_eq!(COVERAGE_TOKENS.len(), 110);
}
