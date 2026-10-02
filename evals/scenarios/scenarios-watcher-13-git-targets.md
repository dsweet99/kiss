# Mixing `kiss test-watch` with `kiss test`

One `kiss test-watch` process stays up in a worktree. It takes no options and no TARGET. A later `kiss test` contacts it, echoes the reply, and exits. The watcher does not exit when that later command finishes.

"Return" means the watcher replies. It does not mean the watch process exits.

Where this file disagrees with VISION.md, follow this file. VISION.md still treats bare `kiss test` as `kiss test .`, and it still calls the watcher `kiss test --watch`. From a subdirectory, bare `kiss test` covers the whole repository and `kiss test .` covers only that subdirectory. The watcher command is `kiss test-watch`.

Unless a scenario names a different code, the process exits with:

- 0 when every test in scope passed and there was no operational failure
- 1 when any test in scope is FAIL or TIMEOUT and there was no operational failure
- 255 on an operational failure, including a failure to reach the watcher that the command does not recover from by running the tests itself
- 2 on a usage error

Test lines are the same whether or not a watcher is running. They list every FAIL in scope and every TIMEOUT in scope, whether that result was just produced or read from the cache. They also list every test that ran for that request. A cached PASS that did not run has no line of its own. The summary counts every PASS, FAIL, and TIMEOUT in scope. Doctests are never in scope. Messages that only describe contact with the watcher, such as a waiting line, may differ. The exit codes above apply in either case.

A command that is waiting in order to proceed repeats its waiting message every 3 seconds. While `kiss test-watch` is idle and waiting for a connection, it does not repeat a waiting message.

The watcher runs tests on its own cycles: a startup cycle, a cycle after edits settle, and a cycle once it has noticed that a config file changed. A plain client does not start a cycle and does not make the watcher run tests. The watcher answers a client only while it is waiting, from the cache, for that client's scope. It finishes the cycle it is running before it answers a waiting client. A client that arrives during a cycle waits until the watcher is waiting. The in-flight cycle is not filtered or narrowed for that client, and the answer does not include a cycle that has not started. `kiss test --retry-bad` is the exception: when that client is answered, the watcher runs the FAIL and TIMEOUT tests compatible with the client's TARGET. A client interrupted while it is only waiting is dropped. The watcher does not start a cycle for it.

## 13. Git TARGETs (`commit`, `base`, `main`) sent to the watcher

User actions: On a feature branch, with `kiss test-watch` idle after a full-suite cycle, make these changes: commit a change to one source file, leave a second source file edited but uncommitted, add a new untracked test file that is not listed in `.gitignore`, add another untracked file that `.gitignore` does exclude, and delete a tracked test file that already has cached results. Wait until the watcher has finished the cycle for those edits. Then run, one at a time: `kiss test commit`, `kiss test base --base-branch dev` (where `dev` is the branch the feature branched from), and `kiss test main --main-branch trunk` (where `trunk` is the repository's main branch). Also try `kiss test commit --main-branch trunk`.

Correct behavior: The edits settle, and the watcher itself runs the tests those edits require. That cycle is not limited to a later client's git TARGET. Each valid command is answered from the cache when the watcher is waiting. It does not start a cycle. The reply's scope is tests that cover the selected source changes, plus every test defined in a changed, new, or untracked test file that is in scope. Files excluded by `.gitignore` are not in scope. Cached results for a deleted test file are omitted from the lines, the summary, and the exit code.

`kiss test commit` covers the uncommitted edit, the new untracked test file, and the deleted test file. It does not cover the already committed change. `kiss test base --base-branch dev` and `kiss test main --main-branch trunk` cover every in-scope change since the branch's merge-base with `dev` or `trunk`, including the committed change, the uncommitted edit, the new untracked test file, and the deleted test file. The deleted test file adds no tests. Its cached results stay omitted from the lines, the summary, and the exit code. Each command's reply is for its scope only. The watcher keeps the full-suite results for a later bare `kiss test`.

`kiss test commit --main-branch trunk` is rejected before contacting the watcher. It prints `error: kiss test: --main-branch is only valid with kiss test main` and exits with code 2. The watch process keeps running.
