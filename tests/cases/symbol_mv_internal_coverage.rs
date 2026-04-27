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
    run_mv(Language::Python, &format!("{}::C.helper", file.display()), "renamed", tmp.path());
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
    run_mv(Language::Rust, &format!("{}::Y.helper", file.display()), "renamed", tmp.path());
}
