# Mixing `kiss test --watch` with `kiss test`

Correct behavior follows VISION.md: one `kiss test --watch` process stays up in a repository; a later `kiss test` contacts it, echoes the reply, and exits. The watcher does not exit when the later command finishes.

“Return” here means the watcher replies. It does not mean the watch process exits.

## 1. No files changed

User actions: Start `kiss test --watch` and wait until it has finished at least one cycle. Do not edit the tree. In another terminal, run `kiss test` (or `kiss test .`).

Correct behavior: `kiss test` contacts the watcher and asks for results. Because nothing has changed, the watcher does not run tests or coverage again. It immediately replies with the last PASS, FAIL, and TIMEOUT information for the full suite (Python and Rust). `kiss test` prints that recap, uses the matching exit code, and exits. The watch process keeps running.

## 2. Files changed

User actions: Leave `kiss test --watch` running. Edit, add, or delete source that the suite covers. Then run `kiss test` in another terminal. The later command may arrive while the watcher is still waiting for edits to settle, or after they have settled.

Correct behavior: The watcher runs the usual `kiss test` workflow: only the tests that the cache says are needed. When that cycle finishes, it replies. `kiss test` echoes the recap and exit code, then exits. If a cycle is already in flight, `kiss test` may print `kiss test: waiting for watcher (pid …)` and wait for that cycle instead of starting a second one. The watch process keeps running.

## 3. A TARGET while the watcher is on the full suite

User actions: Start `kiss test --watch` with no path operands (the full suite). After it is idle, run `kiss test PATH` or `kiss test PATH::symbol` for one file or test.

Correct behavior: `kiss test` contacts the watcher with that TARGET. The watcher starts a scoped cycle for the TARGET only. The later command’s recap lists that TARGET’s results, not sibling tests. The watcher’s memory of the last full-suite recap is kept for a later unscoped `kiss test`. The watch process keeps running.

## 4. Retry only failing tests in a TARGET

User actions: Leave `kiss test --watch` running on the full suite. Run `kiss test --retry-bad TARGET`, where TARGET is a file or selector that includes tests previously marked FAIL or TIMEOUT.

Correct behavior: `kiss test` contacts the watcher. The watcher starts a scoped cycle for that TARGET and reruns only the FAIL and TIMEOUT tests in it. Passing siblings in the same file are not rerun. `kiss test` echoes that scoped recap and exit code, then exits. The watch process keeps running.

## 5. `--lang rust`, then a bare `kiss test`

User actions: Leave `kiss test --watch` running on the full suite, with no file changes after its last cycle. Run `kiss test --lang rust`. Then run `kiss test` with no `--lang`.

Correct behavior: Both commands contact the watcher and do not start a new test or coverage cycle.

The `--lang rust` command is answered with the cached Rust PASS, FAIL, and TIMEOUT recap only. The later bare `kiss test` is answered with the cached full-suite recap (Python and Rust). Each command prints its recap, uses the matching exit code, and exits. The watch process keeps running.


## 6. `--lang python`, then a bare `kiss test`

User actions: Leave `kiss test --watch` running on the full suite, with no file changes after its last cycle. Run `kiss test --lang python`. Then run `kiss test` with no `--lang`.

Correct behavior: Both commands contact the watcher and do not start a new test or coverage cycle.

The `--lang python` command is answered with the cached Python PASS, FAIL, and TIMEOUT recap only. The later bare `kiss test` is answered with the cached full-suite recap (Python and Rust). Each command prints its recap, uses the matching exit code, and exits. The watch process keeps running.


