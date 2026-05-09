//! Parse-cache regression for review findings against parser-first `kiss mv`.
//! Sibling of `review_findings.rs`; split per `lines_per_file` advice
//! in `.llm_style/style.md`.

use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

/// Bug: `cached_parse` keys the per-invocation parse cache on
/// `(content.as_ptr(), content.len(), language)`. Inside `plan_edits`, each
/// candidate file in `append_reference_edits` is read into a transient
/// `String` that is dropped at the end of the loop iteration. On Linux glibc
/// (and most allocators), the next `String` of identical capacity is almost
/// always handed the freed slot — so the next file's pointer collides with
/// the previous file's pointer and length. The cache returns the previous
/// file's `AstResult`, whose `start/end` offsets are then matched against
/// the new file's bytes. Sites that don't line up are silently dropped, so
/// real call sites in the second file are not renamed.
///
/// The construction below pads `b.py` and `c.py` to identical byte lengths.
/// `b.py` puts `helper()` at one column; `c.py` puts `helper()` at a
/// different column with a non-`helper` identifier sitting at b's offset.
/// With the cache bug, c's `helper()` is missed.
/// Code ref: `src/symbol_mv_support/ast_plan.rs::cached_parse` (key uses
/// `content.as_ptr() as usize`, no content hash).
#[test]
fn review_parse_cache_must_not_collide_on_pointer_reuse_across_files() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("a_src.py");
    let b = tmp.path().join("b.py");
    let c = tmp.path().join("c.py");
    fs::write(
        &src,
        "\
def helper():
    return 1
",
    )
    .unwrap();

    let b_body = "\
def b_caller():
    x = helper()
    return x
";
    let c_body = "\
def c_caller():
    return helper()
";
    assert!(
        b_body.len() != c_body.len(),
        "sanity: bodies must differ in length to expose offset collision"
    );
    let (shorter, longer, b_is_shorter) = if b_body.len() < c_body.len() {
        (b_body, c_body, true)
    } else {
        (c_body, b_body, false)
    };
    let pad = "# ".to_string() + &"x".repeat(longer.len() - shorter.len() - 3) + "\n";
    let padded_shorter = format!("{shorter}{pad}");
    assert_eq!(
        padded_shorter.len(),
        longer.len(),
        "padding must equalize lengths so allocator-reuse triggers cache collision"
    );
    if b_is_shorter {
        fs::write(&b, padded_shorter).unwrap();
        fs::write(&c, longer).unwrap();
    } else {
        fs::write(&c, padded_shorter).unwrap();
        fs::write(&b, longer).unwrap();
    }

    let opts = MvOptions {
        query: format!("{}::helper", src.display()),
        new_name: "renamed".to_string(),
        paths: vec![tmp.path().display().to_string()],
        to: None,
        dry_run: false,
        json: false,
        lang_filter: Some(Language::Python),
        ignore: vec![],
    };
    assert_eq!(run_mv_command(opts), 0);

    let updated_b = fs::read_to_string(&b).unwrap();
    let updated_c = fs::read_to_string(&c).unwrap();
    assert!(
        updated_b.contains("renamed()"),
        "b.py call site must be renamed; got:\n{updated_b}"
    );
    assert!(
        updated_c.contains("renamed()"),
        "c.py call site must be renamed; got:\n{updated_c}"
    );
}
