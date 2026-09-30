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

## 21. Config files change while the watcher is running

User actions: Start `kiss test-watch` and wait until it is idle. Change `.kissconfig`, then `pyproject.toml`, then `Cargo.toml`, one at a time. After each change, run `kiss test` with no source edits. Separately, start a cycle and, while that cycle is still running, change one of those three files. Let that cycle finish, then run `kiss test` again with no source edits. Restart is not part of this scenario.

Correct behavior: The watcher keeps the configs it had when it started. It does not reload them. On every transition into running, and on every transition into waiting, it warns that it is running with outdated configs. The warning is tied to the transition. It is not repeated every 3 seconds while the watcher sits idle waiting for a connection. The watcher includes that warning in the reply. The client prints the warning on its own stderr, along with the usual reply on stdout. The watch process keeps running until it is stopped.

Each of those config changes invalidates the cache. A `kiss test` started after the watcher is idle does not reuse PASS, FAIL, or TIMEOUT results cached before that change. It runs the tests in its scope again, still under the configs the watcher started with. Ignore prefixes from the `.kissconfig` the watcher started with still exclude paths. A later edit to those prefixes does not change the list the watcher is using. There is no `--ignore` option.

A cycle that is already running when the file changes finishes, reports, and records results as usual. The next request treats the cache as invalid, including those just-recorded results. It runs the tests in its scope again, still under the configs the watcher started with.
