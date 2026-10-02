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

## 11. Starting `kiss test-watch` while a plain `kiss test` is running

User actions: Make sure no watcher is running. Edit source so some tests need to run, then run `kiss test`. While it is still running tests, start `kiss test-watch` in another terminal. Leave the plain run going for more than two minutes, so it is still holding the lock after 90 seconds.

Correct behavior: The two processes lock each other out. The watcher waits with no time limit. After 90 seconds it is still waiting. It does not give up, it does not start its first cycle while the plain `kiss test` holds the lock, and it does not report "already running" for a plain `kiss test`. While it waits, it prints `kiss test-watch: waiting for kiss test` and repeats that line every 3 seconds. The plain `kiss test` finishes, prints its results, and exits. That plain run uses the same test lines as a reply from the watcher. The watcher then takes the lock and starts. Its first cycle reuses the results and coverage the plain command just recorded, so it runs only what is still needed. If there were no further edits, it runs nothing, including cached FAIL and TIMEOUT tests. After that, a later `kiss test` is answered by the watcher as in scenario 1. The watch process keeps running.
