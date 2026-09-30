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

## 10. The watcher was interrupted with CTRL-C or killed

User actions: Start `kiss test-watch`. Edit source so a cycle begins, and while tests are running, stop the watcher with CTRL-C (or `kill -9` its pid). The session file and socket may be left behind. Then run `kiss test`. After that, start `kiss test-watch` again.

Correct behavior: The watcher exits quickly on CTRL-C and does no cleanup. The later `kiss test` notices that the recorded watcher is no longer alive, clears the stale session, and runs the usual workflow itself under the normal lock. It does not hang, and it does not treat the dead watcher as a connection failure, so this recovery is not exit 255. It reuses results that were recorded before the interruption and runs only what is still needed. A Rust batch that was in progress and had not been recorded may be run again. Recorded FAIL and TIMEOUT tests are not run again merely because they failed. It prints its results and exits with 0 or 1. That plain run uses the same test lines as a reply from the watcher. The new `kiss test-watch` starts normally: it replaces the stale socket and session, runs a first cycle that reuses the cache the same way, and then serves later `kiss test` commands as in scenario 1.
