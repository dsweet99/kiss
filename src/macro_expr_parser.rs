use syn::parse::{Parse, ParseStream};
use syn::{Expr, Result, Token};

pub(crate) fn parse_single_expr(tokens: &proc_macro2::TokenStream) -> Option<Expr> {
    syn::parse2::<Expr>(tokens.clone()).ok()
}

pub(crate) fn parse_expr_list(tokens: &proc_macro2::TokenStream) -> Option<Vec<Expr>> {
    #[derive(Clone)]
    struct ExprList(Vec<Expr>);

    impl Parse for ExprList {
        fn parse(input: ParseStream) -> Result<Self> {
            let mut exprs = Vec::new();
            while !input.is_empty() {
                exprs.push(input.parse::<Expr>()?);
                if input.peek(Token![,]) {
                    let _: Token![,] = input.parse()?;
                }
            }
            Ok(Self(exprs))
        }
    }

    syn::parse2::<ExprList>(tokens.clone())
        .ok()
        .map(|ExprList(exprs)| exprs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn parse_single_expr_accepts_binary() {
        let tokens = proc_macro2::TokenStream::from_str("1 + 2").unwrap();
        assert!(parse_single_expr(&tokens).is_some());
    }

    #[test]
    fn parse_expr_list_accepts_comma_separated() {
        let tokens = proc_macro2::TokenStream::from_str("a, b").unwrap();
        let exprs = parse_expr_list(&tokens).expect("comma-separated expr list");
        assert_eq!(exprs.len(), 2);
    }

    #[test]
    fn parse_expr_list_empty_and_single() {
        let empty = proc_macro2::TokenStream::from_str("").unwrap();
        assert_eq!(parse_expr_list(&empty).unwrap().len(), 0);
        let one = proc_macro2::TokenStream::from_str("solo").unwrap();
        assert_eq!(parse_expr_list(&one).unwrap().len(), 1);
    }
}
