use proc_macro2::{TokenStream, TokenTree};

use super::cfg_pred::{ATOM_TEST, AtomInterner, CfgPred};
use super::error::RoleBuildError;
use std::path::Path;

pub fn parse_cfg_tokens(
    tokens: TokenStream,
    atoms: &mut AtomInterner,
    path: &Path,
) -> Result<CfgPred, RoleBuildError> {
    let parts = split_comma(tokens);
    if parts.is_empty() {
        return Err(malformed(path, "empty cfg"));
    }
    if parts.len() == 1 {
        return parse_one(parts.into_iter().next().unwrap_or_default(), atoms, path);
    }
    let mut parsed = Vec::new();
    for part in parts {
        parsed.push(parse_one(part, atoms, path)?);
    }
    Ok(CfgPred::all(parsed))
}

fn parse_one(
    tokens: TokenStream,
    atoms: &mut AtomInterner,
    path: &Path,
) -> Result<CfgPred, RoleBuildError> {
    let mut iter = tokens.into_iter().peekable();
    let Some(first) = iter.next() else {
        return Err(malformed(path, "empty cfg atom"));
    };
    match first {
        TokenTree::Ident(ident) => parse_ident(ident, &mut iter, atoms, path),
        TokenTree::Group(group) => parse_cfg_tokens(group.stream(), atoms, path),
        _ => Err(malformed(path, "unexpected cfg token")),
    }
}

fn parse_ident(
    ident: proc_macro2::Ident,
    iter: &mut std::iter::Peekable<impl Iterator<Item = TokenTree>>,
    atoms: &mut AtomInterner,
    path: &Path,
) -> Result<CfgPred, RoleBuildError> {
    let name = ident.to_string();
    match name.as_str() {
        "all" => Ok(CfgPred::all(parse_group_parts(iter, atoms, path)?)),
        "any" => Ok(CfgPred::any(parse_group_parts(iter, atoms, path)?)),
        "not" => {
            let mut parts = parse_group_parts(iter, atoms, path)?;
            if parts.len() != 1 {
                return Err(malformed(path, "not() expects one operand"));
            }
            Ok(CfgPred::not(parts.pop().unwrap_or(CfgPred::True)))
        }
        _ => parse_atom_or_kv(&name, iter, atoms),
    }
}

fn parse_atom_or_kv(
    name: &str,
    iter: &mut std::iter::Peekable<impl Iterator<Item = TokenTree>>,
    atoms: &mut AtomInterner,
) -> Result<CfgPred, RoleBuildError> {
    if matches!(iter.peek(), Some(TokenTree::Punct(p)) if p.as_char() == '=') {
        iter.next();
        let value = match iter.next() {
            Some(TokenTree::Literal(lit)) => lit.to_string(),
            _ => {
                return Err(RoleBuildError::MalformedCfg {
                    path: std::path::PathBuf::from("<cfg>"),
                    message: format!("expected literal after {name}="),
                });
            }
        };
        let key = format!("{name}={value}");
        return Ok(CfgPred::Atom(atoms.intern(&key)));
    }
    if name == "test" {
        return Ok(CfgPred::Atom(ATOM_TEST));
    }
    Ok(CfgPred::Atom(atoms.intern(name)))
}

fn parse_group_parts(
    iter: &mut std::iter::Peekable<impl Iterator<Item = TokenTree>>,
    atoms: &mut AtomInterner,
    path: &Path,
) -> Result<Vec<CfgPred>, RoleBuildError> {
    let Some(TokenTree::Group(group)) = iter.next() else {
        return Err(malformed(path, "expected (...) after cfg operator"));
    };
    let mut out = Vec::new();
    for part in split_comma(group.stream()) {
        out.push(parse_one(part, atoms, path)?);
    }
    Ok(out)
}

fn split_comma(tokens: TokenStream) -> Vec<TokenStream> {
    let mut parts = Vec::new();
    let mut current = TokenStream::new();
    for token in tokens {
        if matches!(&token, TokenTree::Punct(p) if p.as_char() == ',') {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
        } else {
            current.extend([token]);
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

fn malformed(path: &Path, message: &str) -> RoleBuildError {
    RoleBuildError::MalformedCfg {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

pub(crate) fn take_until_comma(iter: &mut impl Iterator<Item = TokenTree>) -> TokenStream {
    let mut out = TokenStream::new();
    for token in iter.by_ref() {
        if matches!(&token, TokenTree::Punct(p) if p.as_char() == ',') {
            break;
        }
        out.extend([token]);
    }
    out
}

#[cfg(test)]
mod parse_test {
    use super::*;
    use std::path::Path;
    use std::str::FromStr;

    #[test]
    fn parses_test_all_any_not() {
        let mut atoms = AtomInterner::new();
        let path = Path::new("x.rs");
        let pred =
            parse_cfg_tokens(TokenStream::from_str("test").unwrap(), &mut atoms, path).unwrap();
        assert_eq!(pred, CfgPred::Atom(ATOM_TEST));
        let pred = parse_cfg_tokens(
            TokenStream::from_str("all(test, unix)").unwrap(),
            &mut atoms,
            path,
        )
        .unwrap();
        assert!(matches!(pred, CfgPred::All(_)));
        let pred = parse_cfg_tokens(
            TokenStream::from_str("any(test, feature = \"x\")").unwrap(),
            &mut atoms,
            path,
        )
        .unwrap();
        assert!(matches!(pred, CfgPred::Any(_)));
        let pred = parse_cfg_tokens(
            TokenStream::from_str("not(test)").unwrap(),
            &mut atoms,
            path,
        )
        .unwrap();
        assert!(matches!(pred, CfgPred::Not(_)));
    }

    #[test]
    fn empty_cfg_is_malformed() {
        let mut atoms = AtomInterner::new();
        let err = parse_cfg_tokens(TokenStream::new(), &mut atoms, Path::new("x.rs"));
        assert!(err.is_err());
    }
}
