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

## 22. `--retry-bad` with no watcher, and during watcher startup

User actions:

1. Make sure no watcher is running. Start `kiss test` so that it holds the lock. While it runs, in another terminal run `kiss test --retry-bad TARGET`, where TARGET contains a FAIL or TIMEOUT test.
2. After that plain run has finished and no watcher is running, run `kiss test --retry-bad TARGET` on a TARGET whose tests are all PASS, with no file changes.
3. Start `kiss test-watch` where the first cycle will take a while. Before that cycle finishes, run `kiss test --retry-bad TARGET`.

Correct behavior:

1. The `--retry-bad` command takes the normal lock. It prints `kiss test: waiting for kiss test` every 3 seconds while the first command holds the lock. It does not run tests at the same time as the first command. When it gets the lock, there is no watcher to answer it, so this command itself runs the tests. `--retry-bad` adds FAIL and TIMEOUT tests in TARGET to whatever the cache already needs. The cache is not corrupted.
2. Nothing needs to run. The command starts no test run and answers from the cache for TARGET. That plain run uses the same test lines as a reply from the watcher.
3. The client waits, with no overlapping cycle, as in scenario 9. When the watcher is waiting, it runs the FAIL and TIMEOUT tests compatible with TARGET, as in scenario 4. It does not run PASS tests for that client, and it does not answer by filtering the startup cycle. The watch process keeps running.
