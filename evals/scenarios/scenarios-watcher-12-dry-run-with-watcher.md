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

## 12. `kiss test` has no `--dry-run` option

`kiss test` has no `--dry-run` option. `kiss test-watch` has no `--dry-run` option either, because `kiss test-watch` accepts no options at all. Neither command has a dry-run mode, and the watcher has nothing to accept or reject for one.

User actions: With no watcher, run `kiss test --dry-run`, then `kiss test --dry-run PATH`. With `kiss test-watch` already running, run those two commands again, including once while a cycle is in progress.

Correct behavior: `--dry-run` is not a recognized option of `kiss test`. Kiss treats it the way it treats an unknown option such as `--gobledygook`.

With no watcher, each `kiss test --dry-run` command fails as an unknown option. It prints an error, exits with code 2, and does not take the lock or run tests. Cached results do not change.

With a watcher running, each `kiss test --dry-run` command fails the same way, before it contacts the watcher. The watcher sees no request from it. Cached results do not change. The watcher keeps serving later plain `kiss test` commands.
