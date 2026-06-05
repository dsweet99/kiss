use super::*;


fn parse_expr(src: &str) -> (tree_sitter::Tree, String) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(src, None).unwrap();
    (tree, src.to_string())
}

#[test]
fn const_false_detects_false_and_zero() {
    let (tree, src) = parse_expr("if False:\n    x()\n");
    let root = tree.root_node();
    let if_node = root.named_child(0).unwrap();
    let cond = if_node.child_by_field_name("condition").unwrap();
    assert!(is_py_const_false(cond, &src));
}

#[test]
fn dead_if_body_refs_not_collected() {
    let src = "def test_fn():\n    if False:\n        foo()\n    bar()\n";
    let (tree, src) = parse_expr(src);
    let root = tree.root_node();
    let func = root.named_child(0).unwrap();
    let body = func.child_by_field_name("body").unwrap();
    let mut refs = std::collections::HashSet::new();
    collect_py_live_scope(body, &src, &mut |n| {
        if n.kind() == "identifier" {
            let name = &src[n.start_byte()..n.end_byte()];
            if name != "test_fn" {
                refs.insert(name.to_string());
            }
        }
    });
    assert!(refs.contains("bar"));
    assert!(!refs.contains("foo"));
}

#[test]
fn elif_false_skipped_live_elif_taken() {
    let src = "def test_x():\n    if False:\n        dead()\n    elif True:\n        live()\n";
    let (tree, src) = parse_expr(src);
    let func = tree.root_node().named_child(0).unwrap();
    let body = func.child_by_field_name("body").unwrap();
    let mut refs = std::collections::HashSet::new();
    collect_py_live_scope(body, &src, &mut |n| {
        if n.kind() == "identifier" {
            refs.insert(src[n.start_byte()..n.end_byte()].to_string());
        }
    });
    assert!(refs.contains("live"));
    assert!(!refs.contains("dead"));
}

#[test]
fn import_names_do_not_recurse_into_usage() {
    let src = "from mod import fn\n";
    let (tree, src) = parse_expr(src);
    let mut usage = std::collections::HashSet::new();
    collect_py_live_scope(tree.root_node(), &src, &mut |n| {
        if n.kind() == "identifier" {
            usage.insert(src[n.start_byte()..n.end_byte()].to_string());
        }
    });
    assert!(!usage.contains("fn"));
}

#[test]
fn if_false_else_branch_still_live() {
    let src = "def test_x():\n    if False:\n        dead()\n    else:\n        ok()\n";
    let (tree, src) = parse_expr(src);
    let body = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .child_by_field_name("body")
        .unwrap();
    let mut refs = std::collections::HashSet::new();
    collect_py_live_scope(body, &src, &mut |n| {
        if n.kind() == "identifier" {
            refs.insert(src[n.start_byte()..n.end_byte()].to_string());
        }
    });
    assert!(refs.contains("ok"));
    assert!(!refs.contains("dead"));
}

#[test]
fn if_true_consequence_is_live() {
    let src = "def test_x():\n    if True:\n        live()\n";
    let (tree, src) = parse_expr(src);
    let body = tree.root_node().named_child(0).unwrap().child_by_field_name("body").unwrap();
    let mut refs = std::collections::HashSet::new();
    collect_py_live_scope(body, &src, &mut |n| {
        if n.kind() == "identifier" {
            refs.insert(src[n.start_byte()..n.end_byte()].to_string());
        }
    });
    assert!(refs.contains("live"));
}

#[test]
fn while_false_body_skipped() {
    let src = "def test_x():\n    while False:\n        dead()\n    ok()\n";
    let (tree, src) = parse_expr(src);
    let body = tree.root_node().named_child(0).unwrap().child_by_field_name("body").unwrap();
    let mut refs = std::collections::HashSet::new();
    collect_py_live_scope(body, &src, &mut |n| {
        if n.kind() == "identifier" {
            refs.insert(src[n.start_byte()..n.end_byte()].to_string());
        }
    });
    assert!(refs.contains("ok"));
    assert!(!refs.contains("dead"));
}

#[test]
fn count_py_live_branches_counts_for_loop() {
    let src = "def test_x():\n    for x in items:\n        pass\n";
    let (tree, src) = parse_expr(src);
    let func = tree.root_node().named_child(0).unwrap();
    let n = count_py_live_branches(func, &src);
    assert!(n >= 1, "expected at least one live branch, got {n}");
}

#[test]
fn const_false_detects_comparison_to_zero() {
    let (tree, src) = parse_expr("if 1 == 0:\n    x()\n");
    let if_node = tree.root_node().named_child(0).unwrap();
    let cond = if_node.child_by_field_name("condition").unwrap();
    assert!(is_py_const_false(cond, &src));
}

#[test]
fn handle_py_if_alternative_block_path() {
    let src = "def test_x():\n    if cond:\n        a()\n    else:\n        b()\n";
    let (tree, src) = parse_expr(src);
    let body = tree.root_node().named_child(0).unwrap().child_by_field_name("body").unwrap();
    let mut refs = std::collections::HashSet::new();
    collect_py_live_scope(body, &src, &mut |n| {
        if n.kind() == "identifier" {
            let name = &src[n.start_byte()..n.end_byte()];
            if name != "test_x" && name != "cond" {
                refs.insert(name.to_string());
            }
        }
    });
    assert!(refs.contains("a"));
    assert!(refs.contains("b"));
}

#[test]
fn handle_py_if_skips_false_elif_chain() {
    let src = "def test_x():\n    if False:\n        dead()\n    elif False:\n        dead2()\n    elif True:\n        live()\n";
    let (tree, src) = parse_expr(src);
    let body = tree.root_node().named_child(0).unwrap().child_by_field_name("body").unwrap();
    let mut refs = std::collections::HashSet::new();
    collect_py_live_scope(body, &src, &mut |n| {
        if n.kind() == "identifier" {
            refs.insert(src[n.start_byte()..n.end_byte()].to_string());
        }
    });
    assert!(refs.contains("live"));
    assert!(!refs.contains("dead"));
    assert!(!refs.contains("dead2"));
}

#[test]
fn handle_py_if_const_false_with_no_else() {
    let src = "def test_x():\n    if False:\n        dead()\n    live()\n";
    let (tree, src) = parse_expr(src);
    let body = tree.root_node().named_child(0).unwrap().child_by_field_name("body").unwrap();
    let mut refs = std::collections::HashSet::new();
    collect_py_live_scope(body, &src, &mut |n| {
        if n.kind() == "identifier" {
            let name = &src[n.start_byte()..n.end_byte()];
            if name != "test_x" {
                refs.insert(name.to_string());
            }
        }
    });
    assert!(refs.contains("live"));
    assert!(!refs.contains("dead"));
}

#[test]
fn eval_py_boolean_and_or() {
    let (tree, src) = parse_expr("if False and False:\n    x()\n");
    let cond = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .child_by_field_name("condition")
        .unwrap();
    assert!(is_py_const_false(cond, &src));
    let (tree2, src2) = parse_expr("if False or False:\n    y()\n");
    let cond2 = tree2
        .root_node()
        .named_child(0)
        .unwrap()
        .child_by_field_name("condition")
        .unwrap();
    assert!(is_py_const_false(cond2, &src2));
}

#[test]
fn direct_all_py_dead_region_helpers() {
    let (tree, src) = parse_expr("if 1 != 0:\n    x()\n");
    let cond = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .child_by_field_name("condition")
        .unwrap();
    assert!(!is_py_const_false(cond, &src));
    let (tree2, src2) = parse_expr("if 0:\n    pass\n");
    let int_node = tree2
        .root_node()
        .named_child(0)
        .unwrap()
        .child_by_field_name("condition")
        .unwrap();
    assert!(is_py_const_false(int_node, &src2));
    let (tree3, src3) = parse_expr("def f():\n    if 0:\n        a()\n    elif 0:\n        b()\n    else:\n        c()\n");
    let body = tree3
        .root_node()
        .named_child(0)
        .unwrap()
        .child_by_field_name("body")
        .unwrap();
    let mut ids = std::collections::HashSet::new();
    collect_py_live_scope(body, &src3, &mut |n| {
        if n.kind() == "identifier" {
            ids.insert(src3[n.start_byte()..n.end_byte()].to_string());
        }
    });
    assert!(ids.contains("c"));
    assert!(!ids.contains("a"));
    assert!(!ids.contains("b"));
}

#[test]
fn direct_handle_py_if_statement_invocation() {
    let src = "if False:\n    dead()\nelse:\n    ok()\n";
    let (tree, src) = parse_expr(src);
    let if_node = tree.root_node().named_child(0).unwrap();
    let mut refs = std::collections::HashSet::new();
    handle_py_if_statement(if_node, &src, &mut |n| {
        if n.kind() == "identifier" {
            refs.insert(src[n.start_byte()..n.end_byte()].to_string());
        }
    });
    assert!(refs.contains("ok"));
    assert!(!refs.contains("dead"));
}

#[test]
fn handle_py_if_live_consequence_only() {
    let src = "def t():\n    if True:\n        live()\n    elif False:\n        dead()\n";
    let (tree, src) = parse_expr(src);
    let body = tree.root_node().named_child(0).unwrap().child_by_field_name("body").unwrap();
    let mut refs = std::collections::HashSet::new();
    collect_py_live_scope(body, &src, &mut |n| {
        if n.kind() == "identifier" {
            refs.insert(src[n.start_byte()..n.end_byte()].to_string());
        }
    });
    assert!(refs.contains("live"));
    assert!(!refs.contains("dead"));
}

#[test]
fn high_branch_handle_py_if_paths() {
    let fixtures = [
        "def t():\n    if False:\n        a()\n    elif False:\n        b()\n    elif True:\n        c()\n",
        "def t():\n    if False:\n        a()\n    else:\n        d()\n",
        "def t():\n    if 0:\n        e()\n    f()\n",
    ];
    for src in fixtures {
        let (tree, src) = parse_expr(src);
        let body = tree
            .root_node()
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        let mut n = 0usize;
        collect_py_live_scope(body, &src, &mut |_| {
            n += 1;
        });
        assert!(n > 0, "expected live nodes for fixture");
    }
}
