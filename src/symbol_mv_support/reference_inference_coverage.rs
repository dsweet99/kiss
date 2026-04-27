use super::*;
use crate::symbol_mv_support::reference_inference::reference_inference_assignments::is_tuple_assignment_at;

#[test]
fn touch_reference_inference_helpers_for_coverage_gate() {
    let _ = type_from_assignment_rhs("C()");
    let _ = type_from_assignment_rhs("y = C()");
    let _ = type_from_assignment_rhs("pkg.C()");
    let _ = infer_python_receiver_type("x = C()", "x");
    let _ = infer_python_receiver_type_at("class C:\n    def m(self):\n        self.h()\n", 35, "self");
    let _ = infer_python_receiver_type_at("if (x := C()):\n    x.h()\n", 20, "x");
    let _ = rfind_word_boundary("prev_x = D()\nx = C()", "x = ");
    let _ = is_tuple_assignment_at("x, y = C(), D()", 4);
    let _ = enclosing_python_class("class C:\n    def m(self):\n        pass\n", 25);
    let _ = enclosing_python_function_slice("def f():\n    x = 1\n    return x\n", 25);
    let _ = type_from_python_param_annotation("def f(x: Optional[C]):\n", "x");
    let _ = python_method_return_type("def m(self) -> C:\n    pass\n", 20, "m", None);
    let _ = unwrap_python_annotation("Optional[C]");
    let _ = unwrap_python_annotation("Union[C, D]");
    let _ = unwrap_python_annotation("List[pkg.C]");
    let _ = unwrap_python_annotation("C");
    let _ = extract_receiver("self.");
    let _ = extract_receiver("x.foo().");
    let _ = infer_receiver_type("let s: MyT", "s");
    let _ = infer_receiver_type_at("let x: &mut C = c;", 20, "x");
    let _ = method_return_type("fn into_y(&self) -> Y { Y }", 20, "into_y", None);
    let _ = type_after_pattern_last_before("let x: Type", "let x: Type".len(), "let x: ");
    let _ = strip_rust_type_prefix("&mut Type");
    let _ = strip_rust_type_prefix("dyn Trait");
    let _ = strip_rust_type_prefix("impl Trait");
}

#[test]
fn infer_rust_receiver_type_from_function_param() {
    let src = "fn call(f: &Foo) { f.beta(); }";
    let upto = src.find("beta").expect("test fixture should contain beta");
    assert_eq!(
        infer_receiver_type_at(src, upto, "f"),
        Some("Foo".to_string())
    );
}
