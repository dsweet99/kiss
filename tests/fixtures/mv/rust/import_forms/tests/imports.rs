use mv_import_forms::source as source_mod;
use mv_import_forms::source::{self, exported_fn};

#[test]
fn import_forms_work() {
    assert_eq!(exported_fn(4), 5);
    assert_eq!(source_mod::exported_fn(5), 6);
    assert_eq!(source::exported_fn(6), 7);
}
