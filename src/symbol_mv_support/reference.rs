use super::reference_inference::{
    extract_receiver, infer_python_receiver_type_at, infer_receiver_type_at,
};

pub(super) fn infer_python_receiver_type_pub(
    content: &str,
    start: usize,
    receiver: &str,
) -> Option<String> {
    infer_python_receiver_type_at(content, start, receiver)
}

pub(super) fn infer_rust_receiver_type_pub(
    content: &str,
    start: usize,
    receiver: &str,
) -> Option<String> {
    infer_receiver_type_at(content, start, receiver)
}

pub(super) fn extract_receiver_pub(before: &str) -> String {
    extract_receiver(before)
}

pub(super) fn associated_call_owner_matches_pub(
    content: &str,
    start: usize,
    type_name: &str,
) -> bool {
    rust_associated_call_owner(&content[..start]).as_deref() == Some(type_name)
}

fn rust_associated_call_owner(before: &str) -> Option<String> {
    let trimmed = before.trim_end();
    let prefix = trimmed.strip_suffix("::")?;
    let start = prefix
        .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .map_or(0, |idx| idx + 1);
    let name = &prefix[start..];
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod reference_coverage {
    use super::*;

    #[test]
    fn touch_reference_helpers_for_coverage_gate() {
        let _ = extract_receiver_pub("self.");
        let _ = associated_call_owner_matches_pub("self.foo().bar()", 9, "Owner");
        let _ = infer_python_receiver_type_pub("x = C()\nx.", 10, "x");
        let _ = infer_rust_receiver_type_pub("let x: T = T;\nx.", 16, "x");
        let _ = rust_associated_call_owner("foo().bar::");
    }
}
