#![cfg(unix)]

use crate::support::scenario::{
    Reply, Scenario, assert_waits_for_watcher, kiss, skip_under_coverage,
};

const ONE_LANG: &str = "✗ 2 passed · 1 failed · 0 timed out";
const BOTH_LANGS: &str = "✗ 4 passed · 2 failed · 0 timed out";

struct Langs {
    asked: &'static str,
    edited_file: &'static str,
    from: &'static str,
    to: &'static str,
    edit_runs: &'static [&'static str],
}

fn assert_cached(reply: &Reply, summary: &str, phase: &str) {
    assert_eq!(reply.code, Some(1), "{phase}: {reply:?}");
    assert_eq!(reply.summary(), summary, "{phase}: reply scope; {reply:?}");
    assert!(
        !reply.stdout.contains("PASS"),
        "{phase}: the client ran nothing, so no PASS lines; {reply:?}"
    );
}

fn assert_idle_lang_reply(s: &Scenario, lang: &str, phase: &str) {
    let starts = s.starts();
    let reply = kiss(s.root(), &["test", "--lang", lang]);
    assert_cached(&reply, ONE_LANG, phase);
    assert!(s.take_runs().is_empty(), "{phase}: the client runs nothing");
    assert_eq!(s.starts(), starts, "{phase}: the client starts no cycle");
}

fn bilingual_watch_scenario(langs: &Langs) {
    let s = Scenario::new();
    s.python_with_slow(0.5);
    s.rust_with_slow();
    s.commit();
    let mut watch = s.start_watch();
    s.wait_settled();
    assert_eq!(s.take_runs().len(), 6, "startup cycle runs the full suite");

    assert_idle_lang_reply(&s, langs.asked, "idle --lang");
    let starts = s.starts();
    let bare = kiss(s.root(), &["test"]);
    assert_cached(&bare, BOTH_LANGS, "bare after --lang");
    assert!(s.take_runs().is_empty(), "bare client runs nothing");
    assert_eq!(s.starts(), starts, "bare client starts no cycle");

    let edited = s.replace_in(langs.edited_file, langs.from, langs.to);
    s.edit_until_testing(langs.edited_file, &edited);
    let during = kiss(s.root(), &["test", "--lang", langs.asked]);
    assert_waits_for_watcher(&during, "--lang during the other language's cycle");
    assert_cached(
        &during,
        ONE_LANG,
        "--lang during the other language's cycle",
    );
    s.wait_settled();
    assert_eq!(
        s.take_runs(),
        langs.edit_runs,
        "the edit cycle runs the edited language"
    );
    assert_eq!(s.starts(), starts + 1, "the client starts no cycle");

    assert_idle_lang_reply(&s, langs.asked, "--lang after the cycle");
    assert!(watch.still_running(), "watcher must keep running");
}

#[test]
fn lang_rust_is_answered_from_cache_while_python_edits_cycle() {
    if skip_under_coverage() {
        return;
    }
    bilingual_watch_scenario(&Langs {
        asked: "rust",
        edited_file: "lib_a.py",
        from: "return 0\n",
        to: "return 0 + 0\n",
        edit_runs: &["test_pass", "test_slow"],
    });
}

#[test]
fn lang_python_is_answered_from_cache_while_rust_edits_cycle() {
    if skip_under_coverage() {
        return;
    }
    bilingual_watch_scenario(&Langs {
        asked: "python",
        edited_file: "tests/it.rs",
        from: "\n#[test]\nfn rs_pass()",
        to: "\n#[test]\nfn rs_new() {\n    mark(\"rs_new\");\n    std::thread::sleep(std::time::Duration::from_secs(u64::from(demo::value()) * 4));\n}\n\n#[test]\nfn rs_pass()",
        edit_runs: &["rs_fail", "rs_new", "rs_pass", "rs_slow"],
    });
}
