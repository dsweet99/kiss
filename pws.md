# Problems Worth Solving (kiss)

No open PWS in this file (taxonomy: `~/problems_worth_solving.md`).
Prior finds in this IML (host-parallelism budgeting; dual lang/ignore scope
carriers / `WatchPathFilter`; watcher↔client peer protocol; `EnsurePolicy`
bool bag; language-keyed extras *surface*; watch seed∥nudge;
report reuse identity keys only rust extras) — fixed earlier.

---

## 1. Report reuse identity keys only rust extras — FIXED

**Categories:** Hidden invariant; Diffuse concept; Leaky abstraction.

**Problem:** Run/watch surfaces now carry `extras: LanguageKeyed<…>` (PWS #5
fixed). Report cache identity did not follow: `ReportSnapshot.extra` was still
a rust-only `Vec<String>`, `assemble_report` copied only `args.extras.rust`
into it, and `load_ready_for_request` / `ensure_target_report_query` took a
rust-only `&[String]` and compared against `snapshot.extra`. Python extras
still affected planning/collect but never entered the reuse key.

**Fix:** `ReportSnapshot.extras` is now `LanguageKeyed<Vec<String>>`.
`assemble_report` stores both languages; ensure/bind/store identity APIs take
`LanguageKeyed<&[String]>` and include both in the store key (schema
`target-report-store-v3`). Callers pass full `args.extras` / `q.extras.as_slices()`.
