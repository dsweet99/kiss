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

## 18. CTRL-C of a client

User actions:

1. With `kiss test-watch` idle, edit a source file and run `kiss test` so the watcher starts a cycle for that client. While tests for that request are running, press CTRL-C in the client terminal only.
2. Start a cycle that will take a while. While it is running, start a second `kiss test` and leave it only waiting in line, so its own cycle has not started. Press CTRL-C in that second client only.

Correct behavior:

1. The client stops. The watcher does not cancel the cycle and does not treat the client's departure as a special case. It finishes that cycle and records the results as usual, then goes back to waiting for a connection. A later `kiss test` is served normally, and the results recorded for that cycle are available as cache. The watch process keeps running.
2. The watcher drops the second request. When the earlier cycle finishes, it does not start a cycle for the client that was interrupted while waiting. It goes back to waiting for a connection. A later `kiss test` is served normally. The watch process keeps running.
