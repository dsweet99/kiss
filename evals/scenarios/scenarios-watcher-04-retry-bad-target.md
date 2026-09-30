# Mixing `kiss test-watch` with `kiss test`

One `kiss test-watch` process stays up in a repository. It takes no options and no TARGET. A later `kiss test` contacts it, echoes the reply, and exits. The watcher does not exit when that later command finishes.

"Return" means the watcher replies. It does not mean the watch process exits.

Where this file disagrees with VISION.md, follow this file. VISION.md still treats bare `kiss test` as `kiss test .`, and it still calls the watcher `kiss test --watch`. From a subdirectory, bare `kiss test` covers the whole repository and `kiss test .` covers only that subdirectory. The watcher command is `kiss test-watch`.

Unless a scenario names a different code, the process exits with:

- 0 when every test in scope passed and there was no operational failure
- 1 when any test in scope is FAIL or TIMEOUT and there was no operational failure
- 255 on an operational failure, including a failure to reach the watcher that the command does not recover from by running the tests itself
- 2 on a usage error

Test lines are the same whether or not a watcher is running. They list every FAIL in scope and every TIMEOUT in scope, whether that result was just produced or read from the cache. They also list every test that ran for that request. A cached PASS that did not run has no line of its own. The summary counts every PASS, FAIL, and TIMEOUT in scope. Doctests are never in scope. Messages that only describe contact with the watcher, such as a waiting line, may differ. The exit codes above apply in either case.

A command that is waiting in order to proceed repeats its waiting message every 3 seconds. While `kiss test-watch` is idle and waiting for a connection, it does not repeat a waiting message.

The watcher finishes the request it is serving before it starts the next one. A client that arrives during a cycle does not receive that cycle's results. Its own request is handled afterward.

## 4. `--retry-bad` enlarges the set of tests to run

User actions: Leave `kiss test-watch` idle. Then:

1. Run `kiss test --retry-bad TARGET`, where TARGET contains at least one FAIL or TIMEOUT test and at least one PASS test, and no file has changed.
2. Run `kiss test --retry-bad TARGET` where every cached test in TARGET is PASS, and no file has changed.
3. Edit a source file so that a PASS test in TARGET needs to run, while a FAIL test in TARGET would not be selected by that edit alone. Run `kiss test --retry-bad TARGET`.

Correct behavior: `--retry-bad` adds the FAIL and TIMEOUT tests in TARGET to the set that would run for that TARGET anyway. It does not drop tests the edits require. The reply's scope is TARGET, so the summary counts passing tests in TARGET that did not run.

1. The watcher runs the FAIL and TIMEOUT tests in TARGET. It does not run the passing tests.
2. TARGET contributes no FAIL or TIMEOUT tests, and nothing else needs to run. The watcher starts no cycle and replies from the cache.
3. The watcher runs both the PASS test required by the edit and the FAIL test added by `--retry-bad`. The watch process keeps running.
