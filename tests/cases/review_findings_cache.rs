use kiss::Language;
use kiss::symbol_mv::{MvOptions, run_mv_command};
use std::fs;
use tempfile::TempDir;

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
        language_tables: Default::default(),
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
