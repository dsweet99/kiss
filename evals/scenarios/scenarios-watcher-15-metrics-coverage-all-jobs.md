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

## 15. `kiss test-watch` takes no options and no TARGET

`kiss test` has no `--dry-run` option. `kiss test-watch` has no `--dry-run` option because that command accepts no options at all. `--dry-run`, `--metrics`, `--coverage-all`, and `--ignore` are not recognized options of `kiss test` or of `kiss test-watch`. Kiss treats each one the way it treats an unknown option such as `--gobledygook`. `--config` and `-j` remain options of `kiss test` only. Ignore prefixes already written in `.kissconfig` still exclude paths.

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
- `kiss test-watch tests/unit`
- `kiss test-watch commit`
- `kiss test --watch`

With a watcher already running, also run:

- `kiss test --config ci.kissconfig`
- `kiss test -j 4`
- `kiss test --metrics`
- `kiss test --coverage-all`
- `kiss test --ignore tests/unit/slow`
- `kiss test --dry-run`

Separately, add an ignore prefix to `.kissconfig` that excludes a test path. With no watcher, run `kiss test` and do not pass `--ignore`. Then start `kiss test-watch` and run `kiss test` again.

Correct behavior: With no watcher, `kiss test --config ci.kissconfig` and `kiss test -j 4` are accepted. Each one runs the normal `kiss test` workflow. They are not usage errors.

With no watcher, `kiss test --metrics`, `kiss test --coverage-all`, `kiss test --ignore tests/unit/slow`, and `kiss test --dry-run` are not recognized. Each fails as an unknown option, prints an error, exits with code 2, and does not take the lock or run tests.

The plain `kiss test` that has an ignore prefix in `.kissconfig` still excludes that path. The excluded tests do not appear in the lines or the summary. There is no `--ignore` option. The same prefix still excludes that path for a watcher started while that `.kissconfig` is in place.

Each `kiss test-watch` command in the no-watcher list is a usage error. The command accepts no options and no TARGET, so the option is not accepted, or the TARGET is not accepted. It prints an error, exits with code 2, and does not start a watcher, take the lock, or run tests. A following bare `kiss test-watch` still starts cleanly.

`kiss test --watch` is not recognized. `--watch` is not an option. The command prints an error, exits with code 2, and does not start a watcher.

While a watcher is running, `kiss test --config ci.kissconfig` and `kiss test -j 4` contact the watcher. The watcher rejects the request. The client prints an error, exits with code 2, and does not run tests. The running watcher keeps serving later plain `kiss test` commands.

While a watcher is running, `kiss test --metrics`, `kiss test --coverage-all`, `kiss test --ignore tests/unit/slow`, and `kiss test --dry-run` are not recognized. Each fails as an unknown option before it contacts the watcher. The watcher sees no request from it. Each prints an error, exits with code 2, and does not run tests. The running watcher keeps serving later plain `kiss test` commands.
