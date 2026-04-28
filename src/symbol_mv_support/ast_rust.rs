#[path = "ast_rust_macros.rs"]
mod ast_rust_macros;
use ast_rust_macros::collect_macro_reference_sites;
#[cfg(test)]
#[path = "ast_rust_test.rs"]
mod ast_rust_test;

include!("ast_rust_body.txt");
