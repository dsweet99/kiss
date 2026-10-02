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

## 15. `kiss test-watch` takes no options and no TARGET

`kiss test-watch` accepts no options and no TARGET. Every option and every TARGET on that command is one usage error: the option is not accepted, or the TARGET is not accepted. That includes `--config`, `-j`, `--lang`, `--retry-bad`, `--dry-run`, `--metrics`, `--coverage-all`, `--ignore`, and `--gobledygook`. A command that carries both an option and a TARGET is still that one error. It is the same error whether or not a watcher is running.

`--config` and `-j` are options of `kiss test` only. `--dry-run`, `--metrics`, `--coverage-all`, and `--ignore` are not options of `kiss test`. `kiss test` treats each of them as an unknown option, the same way it treats `--gobledygook`. `--watch` is not an option of `kiss test`. An ignore prefix written in `.kissconfig` is scenario 23.

User actions: Make sure no watcher is running. Run each of these, one at a time:

- `kiss test --config ci.kissconfig`
- `kiss test -j 4`
- `kiss test --metrics`
- `kiss test --coverage-all`
- `kiss test --ignore tests/unit/slow`
- `kiss test --dry-run`
- `kiss test-watch --config ci.kissconfig`
- `kiss test-watch -j 4`
- `kiss test-watch --metrics`
- `kiss test-watch --coverage-all`
- `kiss test-watch --ignore tests/unit/slow`
- `kiss test-watch --retry-bad tests/unit`
- `kiss test-watch --lang rust`
- `kiss test-watch --dry-run`
- `kiss test-watch --gobledygook`
- `kiss test-watch tests/unit`
- `kiss test-watch commit`
- `kiss test --watch`

With a watcher already running, run each `kiss test-watch` command from that list again. Also run:

- `kiss test --config ci.kissconfig`
- `kiss test -j 4`
- `kiss test --metrics`
- `kiss test --coverage-all`
- `kiss test --ignore tests/unit/slow`
- `kiss test --dry-run`

Correct behavior: With no watcher, `kiss test --config ci.kissconfig` and `kiss test -j 4` are accepted. Each one runs the normal `kiss test` workflow. They are not usage errors.

With no watcher, `kiss test --metrics`, `kiss test --coverage-all`, `kiss test --ignore tests/unit/slow`, and `kiss test --dry-run` are not recognized. Each fails as an unknown option, prints an error, exits with code 2, and does not take the lock or run tests.

Each `kiss test-watch` command in the no-watcher list is a usage error. The command accepts no options and no TARGET, so the option is not accepted, or the TARGET is not accepted. It prints an error, exits with code 2, and does not start a watcher, take the lock, or run tests. A following bare `kiss test-watch` still starts cleanly.

Those same `kiss test-watch` commands are usage errors when a watcher is already running. The command fails while reading its arguments. It prints that the option is not accepted, or that the TARGET is not accepted, and exits with code 2. It does not print that a watcher is already running. The running watcher sees no request from it and keeps serving later plain `kiss test` commands.

`kiss test --watch` is not recognized. `--watch` is not an option. The command prints an error, exits with code 2, and does not start a watcher.

While a watcher is running, `kiss test --config ci.kissconfig` and `kiss test -j 4` contact the watcher. The watcher rejects the request. The client prints an error, exits with code 2, and does not run tests. The running watcher keeps serving later plain `kiss test` commands.

While a watcher is running, `kiss test --metrics`, `kiss test --coverage-all`, `kiss test --ignore tests/unit/slow`, and `kiss test --dry-run` are not recognized. Each fails as an unknown option before it contacts the watcher. The watcher sees no request from it. Each prints an error, exits with code 2, and does not run tests. The running watcher keeps serving later plain `kiss test` commands.
