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

## 11. Starting `kiss test-watch` while a plain `kiss test` is running

User actions: Make sure no watcher is running. Edit source so some tests need to run, then run `kiss test`. While it is still running tests, start `kiss test-watch` in another terminal. Leave the plain run going longer than a minute.

Correct behavior: The two processes lock each other out. The watcher waits with no time limit. It does not start its first cycle while the plain `kiss test` holds the lock, and it does not report "already running" for a plain `kiss test`. While it waits, it repeats a waiting message every 3 seconds. The plain `kiss test` finishes, prints its results, and exits. That plain run uses the same test lines as a reply from the watcher. The watcher then takes the lock and starts. Its first cycle reuses the results and coverage the plain command just recorded, so it runs only what is still needed. If there were no further edits, it runs nothing, including cached FAIL and TIMEOUT tests. After that, a later `kiss test` is answered by the watcher as in scenario 1. The watch process keeps running.
