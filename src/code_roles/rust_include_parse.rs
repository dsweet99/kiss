use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use syn::parse::Parser;
use syn::{Block, Expr, File, Pat, Type};

use super::error::RoleBuildError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncludeKind {
    Items,
    Statements,
    Expr,
    #[allow(dead_code)]
    Type,
    #[allow(dead_code)]
    Pattern,
}

#[allow(dead_code)]
pub enum IncludeAst {
    File(File),
    Stmts(Vec<syn::Stmt>),
    Expr(Expr),
    Type(Type),
    Pat(Pat),
}

pub fn parse_include_source(
    path: &Path,
    source: &str,
    kind: IncludeKind,
) -> Result<IncludeAst, RoleBuildError> {
    match kind {
        IncludeKind::Items => parse_items(path, source),
        IncludeKind::Statements => parse_stmts(path, source),
        IncludeKind::Expr => parse_one(path, source, IncludeAst::Expr),
        IncludeKind::Type => parse_one(path, source, IncludeAst::Type),
        IncludeKind::Pattern => parse_pat(path, source),
    }
}

fn parse_items(path: &Path, source: &str) -> Result<IncludeAst, RoleBuildError> {
    syn::parse_file(source)
        .map(IncludeAst::File)
        .or_else(|_| parse_stmts(path, source))
        .map_err(|err| rust_parse_err(path, &err.to_string()))
}

fn parse_stmts(path: &Path, source: &str) -> Result<IncludeAst, RoleBuildError> {
    Block::parse_within
        .parse_str(source)
        .map(IncludeAst::Stmts)
        .map_err(|err| rust_parse_err(path, &err.to_string()))
}

fn parse_one<T, F>(path: &Path, source: &str, wrap: F) -> Result<IncludeAst, RoleBuildError>
where
    T: syn::parse::Parse,
    F: FnOnce(T) -> IncludeAst,
{
    syn::parse_str::<T>(source)
        .map(wrap)
        .map_err(|err| rust_parse_err(path, &err.to_string()))
}

fn parse_pat(path: &Path, source: &str) -> Result<IncludeAst, RoleBuildError> {
    syn::parse::Parser::parse_str(syn::Pat::parse_single, source)
        .map(IncludeAst::Pat)
        .map_err(|err| rust_parse_err(path, &err.to_string()))
}

fn rust_parse_err(path: &Path, message: &str) -> RoleBuildError {
    RoleBuildError::RustParse {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

#[cfg(test)]
pub fn read_include(
    path: &Path,
    kind: IncludeKind,
) -> Result<(String, IncludeAst), RoleBuildError> {
    let source = std::fs::read_to_string(path).map_err(|_| RoleBuildError::MissingInclude {
        from: PathBuf::from("<include>"),
        target: path.to_path_buf(),
    })?;
    let ast = parse_include_source(path, &source, kind)?;
    Ok((source, ast))
}

#[cfg(test)]
mod include_parse_test {
    use super::*;

    #[test]
    fn parses_item_and_statement_fragments() {
        let items =
            parse_include_source(Path::new("a.rs"), "pub fn f() {}\n", IncludeKind::Items).unwrap();
        assert!(matches!(items, IncludeAst::File(_)));
        let stmts =
            parse_include_source(Path::new("a.inc"), "let x = 1;\n", IncludeKind::Statements)
                .unwrap();
        assert!(matches!(stmts, IncludeAst::Stmts(_)));
        let expr = parse_include_source(Path::new("e.inc"), "1 + 2", IncludeKind::Expr).unwrap();
        assert!(matches!(expr, IncludeAst::Expr(_)));
        let ty = parse_include_source(Path::new("t.inc"), "i32", IncludeKind::Type).unwrap();
        assert!(matches!(ty, IncludeAst::Type(_)));
        let pat = parse_include_source(Path::new("p.inc"), "x", IncludeKind::Pattern).unwrap();
        assert!(matches!(pat, IncludeAst::Pat(_)));
        assert!(
            read_include(
                Path::new("missing-include-file-xyz.inc"),
                IncludeKind::Items
            )
            .is_err()
        );
    }
}
