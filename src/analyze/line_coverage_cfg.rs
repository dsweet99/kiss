pub(super) fn coverage_off_attrs(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(attr_marks_coverage_off)
}

fn attr_marks_coverage_off(attr: &syn::Attribute) -> bool {
    if attr.path().is_ident("coverage") {
        return matches!(&attr.meta, syn::Meta::List(list) if list.tokens.to_string().replace(' ', "") == "off");
    }
    let Some(doc) = doc_attr_string(attr) else {
        return false;
    };
    doc.contains("kiss-coverage-off")
}

fn doc_attr_string(attr: &syn::Attribute) -> Option<String> {
    if !attr.path().is_ident("doc") {
        return None;
    }
    let syn::Meta::NameValue(nv) = &attr.meta else {
        return None;
    };
    let syn::Expr::Lit(expr_lit) = &nv.value else {
        return None;
    };
    let syn::Lit::Str(s) = &expr_lit.lit else {
        return None;
    };
    Some(s.value())
}
