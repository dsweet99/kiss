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

## 21. Config files change while the watcher is running

User actions: Start `kiss test-watch` and wait until it is idle. Change `.kissconfig`, then `pyproject.toml`, then `Cargo.toml`. With no source edits, run `kiss test` once before the watcher has warned, and once after it has warned and is waiting again. Restart is not part of this scenario.

Correct behavior: The watcher keeps the configs it had when it started. It does not reload them. It notices a config change on its own schedule. Before it has noticed, `kiss test` is answered from the cache it already had, and the reply has no outdated-config warning.

Noticing does not leave a client on that pre-change cache. The watcher runs the tests again itself, under the configs it started with, and it does that before it answers a client who arrives after notice. If it is waiting when it notices, that rerun is the next cycle. If a cycle is already running when it notices, that cycle finishes, and the rerun is the cycle after it. The watcher does not answer in the gap between those two cycles. A client who is already waiting stays waiting until the watcher is waiting after the rerun. The reply is the rerun's cache: that finished cycle, not a cycle that has not started, and not the pre-change cache. The watcher does not reuse PASS, FAIL, or TIMEOUT results cached from before the change. The client does not cause the rerun.

Once it has noticed, every transition into running and every transition into waiting warns that it is running with outdated configs. The warning is tied to the transition. It is not repeated every 3 seconds while the watcher sits idle waiting for a connection. The reply after the rerun includes that warning. The client prints it on stderr, along with the usual reply on stdout. When the watcher is waiting after the rerun, `kiss test` is answered from the cache that rerun recorded.

Ignore prefixes from the `.kissconfig` the watcher started with still exclude paths. A later edit to those prefixes does not change the list the watcher is using. There is no `--ignore` option. The watch process keeps running until it is stopped.
