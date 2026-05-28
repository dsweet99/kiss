use super::references;
use std::collections::HashSet;

#[test]
fn pascal_case_ident_to_snake_edges() {
    let mut refs = HashSet::new();
    let rule_path: syn::Path = syn::parse_str("Rule::ShebangNotExecutable").unwrap();
    references::insert_rule_variant_snake_alias(&rule_path, &mut refs);
    assert!(refs.contains("shebang_not_executable"));
    let no_upper: syn::Path = syn::parse_str("Rule::lowercase").unwrap();
    references::insert_rule_variant_snake_alias(&no_upper, &mut refs);
    assert!(!refs.contains("lowercase"));
}

#[test]
fn collect_rust_references_with_mode_both_modes() {
    let ast: syn::File = syn::parse_str(
        "fn gate() { callee(); }\nfn cal() { let _ = StructLit {}; callee(); }\n",
    )
    .unwrap();
    let mut gate = HashSet::new();
    references::collect_rust_references(&ast, &mut gate);
    assert!(gate.contains("callee"));
    let mut cal = HashSet::new();
    references::collect_rust_references_for_coverage_map(&ast, &mut cal);
    assert!(cal.contains("callee"));
    assert!(cal.contains("StructLit"));
}
