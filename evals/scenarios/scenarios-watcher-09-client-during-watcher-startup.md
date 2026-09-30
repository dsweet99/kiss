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

## 9. `kiss test` while the watcher is still starting

User actions: Start `kiss test-watch` in a repository where its first cycle will take a while (a cold cache, or edited sources). Before that first cycle finishes, run `kiss test` in another terminal. Also try a narrower command, such as `kiss test PATH` or `kiss test --lang rust`, during that same window.

Correct behavior: The client does not start its own test run beside the watcher. It waits, as if queued, and prints `kiss test: waiting for watcher (pid …)`, repeating that line every 3 seconds. The watcher does not start a second, overlapping cycle. The in-flight cycle is not filtered or narrowed to answer the client. There is no dependence between that cycle and the waiting request.

When the watcher finishes the work already under way, it handles the client's request as a normal next request. A bare `kiss test` is then answered by the rules in scenario 1 or scenario 2, whichever matches the tree. A PATH or `--lang` request is answered by the rules in scenario 3 or scenario 5, not by slicing the cycle that just finished. The client prints that reply and exits. The watch process keeps running.
