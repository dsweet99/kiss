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

## 2. Files changed, and a client that arrives during an earlier cycle

User actions: Leave `kiss test-watch` running. Edit, add, or delete source that the suite covers. Run `kiss test` in another terminal. Also start `kiss test` while the watcher is already serving an earlier request. The new command may arrive while the watcher is still waiting for edits to settle, or after they have settled.

Correct behavior: On the new command's own turn, the watcher runs only the tests the cache says are needed for the tree as it is on that turn. A cached FAIL or TIMEOUT is not run again merely because it failed. The client prints the reply for the full suite and exits.

If a cycle is already in progress, the new `kiss test` prints `kiss test: waiting for watcher (pid …)` and repeats that line every 3 seconds. It waits until the watcher has finished the earlier request. It does not print that earlier reply. Edits made during the earlier cycle are visible to this later request. The watch process keeps running.
