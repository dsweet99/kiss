# Problems Worth Solving (kiss)

No open items. Taxonomy: `~/problems_worth_solving.md`.
Prior finds in this IML (host-parallelism budgeting; dual lang/ignore scope
carriers / `WatchPathFilter`; watcher↔client peer protocol; `EnsurePolicy`
bool bag; language-keyed extras *surface*; watch seed∥nudge) — fixed earlier.
Report reuse identity is recorded in the next section. It was fixed earlier.
The five items after that section are fixed in this pass.

---

## Fixed. Report reuse identity keys only rust extras

**Categories:** Hidden invariant; Diffuse concept; Leaky abstraction.

**Problem:** Run/watch surfaces now carry `extras: LanguageKeyed<…>` (PWS #5
fixed). Report cache identity did not follow: `ReportSnapshot.extra` was still
a rust-only `Vec<String>`, `assemble_report` copied only `args.extras.rust`
into it, and `load_ready_for_request` / `ensure_target_report_query` took a
rust-only `&[String]` and compared against `snapshot.extra`. Python extras
still affected planning/collect but never entered the reuse key.

**Fix:** `ReportSnapshot.extras` is `LanguageKeyed<Vec<String>>`.
`assemble_report` stores `args.extras.owned_vecs()`. `load_ready_for_request`
and `ensure_target_report_query` take `LanguageKeyed<&[String]>`, pass that
value into witness loads, and copy it onto `snapshot.extras`. Callers pass
`args.extras`.

---

## Fixed. Rust extras switch a Python-only selector filter

**Categories:** Hidden invariant; Misplaced responsibility.

**Problem:** `apply_runner_extra` returned immediately when `args.extras.rust` was empty, then collected Python node ids and dropped selectors whose text contained `.py`. The same rust-only check raised `NO_SELECTED_TESTS_MSG`. Python extras alone never opened the filter or that error. Rust extras opened a Python collection even when the Python extra list was empty.

**Fix:** The Python collection runs only when Python extras are non-empty, and membership uses `Language::from_path`. `NO_SELECTED_TESTS_MSG` is raised when any language’s extras are non-empty, the scope has no selectors, and `policy.require_complete()` is set.

## Fixed. Selector language is a free string

**Categories:** Diffuse concept; Representable invalid state.

**Problem:** `SelectorRow.language` and `ExecutionWitness.language` were `String`. Snapshot membership used `selector.contains(".py")`. `typed_retry_by_lang` matched the strings `"python"` and `"rust"` and dropped every other spelling.

**Fix:** Both fields are `kiss::Language`. Retry matches the enum. Snapshot membership uses `Language::from_path` on the selector path.

## Fixed. Rust witness load drops runner extras

**Categories:** Hidden invariant; Duplicated policy.

**Problem:** Rust `stored_witness` ignored extras and loaded holding records with `&[]`. Python passed extras into `record_identity`. `stored_witness_matches_extras` defaulted to `extras.is_empty()` while Python returned `true` for every list.

**Fix:** `try_load_rust_execution_witness` takes extras and passes them to `holding_records`. The trait default `stored_witness_matches_extras` is `true` for every language, because each loader applies extras itself. Callers that have no extras pass `&[]`.

## Fixed. Per-language ensure kernels repeat one procedure

**Categories:** Duplicated policy; Scattered change.

**Problem:** `ensure_python_via_kernel` and `ensure_rust_via_kernel` repeated the same steps, and the `jobs > 0` check lived inside the Python helper but outside the Rust helper.

**Fix:** `ensure_language_via_kernel` takes a `Language`, writes that language’s selectors, clears the others, checks `jobs > 0` once, and reads that language’s summary. Both executors call it.

## Fixed. Config language is a second two-variant enum

**Categories:** Diffuse concept; Scattered change.

**Problem:** `ConfigLanguage` duplicated `kiss::Language`. `LanguageTablesPresent` was a public `python` / `rust` bool pair. `missing_language` returned `Option<&'static str>`.

**Fix:** Config load, merge, and validation take `kiss::Language`. `LanguageTablesPresent` stores presence in a `Language`-indexed array and `missing_language` returns `Option<Language>`. Section names come from `Language::label()`.
