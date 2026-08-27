pub fn coverage_off_attrs(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(attr_marks_coverage_off)
}

fn attr_marks_coverage_off(attr: &syn::Attribute) -> bool {
    if attr.path().is_ident("coverage") {
        return matches!(&attr.meta, syn::Meta::List(list) if list.tokens.to_string().replace(' ', "") == "off");
    }
    doc_attr_string(attr).is_some_and(|doc| doc.contains("kiss-coverage-off"))
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

#[cfg(test)]
mod rust_coverage_off_test {
    use super::coverage_off_attrs;

    #[test]
    fn coverage_off_and_doc_marker() {
        let off: syn::ItemFn = syn::parse_str("#[coverage(off)]\nfn f() {}").unwrap();
        let live: syn::ItemFn = syn::parse_str("fn g() {}").unwrap();
        let doc: syn::ItemFn =
            syn::parse_str("/// kiss-coverage-off\nfn h() {}").unwrap();
        assert!(coverage_off_attrs(&off.attrs));
        assert!(!coverage_off_attrs(&live.attrs));
        assert!(coverage_off_attrs(&doc.attrs));
    }
}
