use syn::parse::{Parse, ParseStream};
use syn::{Expr, Result, Token};

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

pub(crate) fn parse_single_expr(tokens: &proc_macro2::TokenStream) -> Option<Expr> {
    syn::parse2::<Expr>(tokens.clone()).ok()
}

pub(crate) fn parse_expr_list(tokens: &proc_macro2::TokenStream) -> Option<Vec<Expr>> {
    syn::parse2::<ExprList>(tokens.clone()).ok().map(|ExprList(exprs)| exprs)
}
