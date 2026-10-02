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

## 7. A second `kiss test-watch` while one is already running

User actions: Start `kiss test-watch` and wait until it is idle. In another terminal in the same worktree, run `kiss test-watch` again. Also run `kiss test-watch tests/unit`, `kiss test-watch --config ci.kissconfig`, `kiss test-watch -j 4`, `kiss test-watch --lang rust`, `kiss test-watch --retry-bad tests/unit`, `kiss test-watch --dry-run`, and `kiss test-watch --gobledygook` while that first watcher is still running. Then run a plain `kiss test`.

Correct behavior: The second bare `kiss test-watch` does not start a session and does not run any tests. It prints `error: kiss test-watch: watcher already running (pid …)` and exits with code 2.

Each of the other commands carries a TARGET or an option. `kiss test-watch` accepts no options and no TARGET, so each one is the usage error in scenario 15. A watcher that is already running does not change that error. The command prints that the option is not accepted, or that the TARGET is not accepted, and exits with code 2. It does not print that a watcher is already running. It does not start a session and does not run any tests.

The first watcher's session, socket, cache, and last results are unchanged. The later plain `kiss test` is answered by the first watcher as in scenario 1. The first watch process keeps running.
