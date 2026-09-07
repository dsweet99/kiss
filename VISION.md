

# kiss test
- Ideally, `kiss test` would be constantly working, use all of the CPUs it was allocated (via num_jobs), show a fairly steady stream of meaningful logging output (so the user knows it's working), and be very efficient with resources.
- Also, the user should be able to kill it at any time with CTRL-C and restart with minimal repeating of work. DON'T try to do cleanup at exit time, though. Just exit on CTRL-C quickly. Defer any housekeeping that might be necessary until the next time 'kiss test' is started.
- When `kiss test --watch` is running, `kiss test` should contact it and ask it for test results.
  - If no files have changed since the watcher's last run, `kiss test --watch` should
    immediately return the pass, fail, & timeout information, and `kiss test` should echo it to stdout and usual with the appropriate summary
    and exit code.
  - If files have changed, the watcher should run the usual `kiss test` workflow -- i.e., run only tests that need to be run based on the cache --
    then return the results for `kiss test` to echo.
- `kiss test --retry-bad TARGET` should rerun whichever tests in the TARGET subset are marked as FAIL or TIMEOUT.

# ops/
- ops/ holds all of the CLI Python scripts the developer might need to work in this repo.
- All ops/ scripts should be exeutable (a+x).
- No __main__ scripts should appear outside of ops/.

# evals/
- Each eval runs in under 60s.
- An eval may publish multiple metrics. Each metric should appear on a line with on of
  these formats:
```
EVAL: metric_name = LARGER(metric_value)
EVAL: metric_name = SMALLER(metric_value)
EVAL: metric_name = PASS
EVAL: metric_name = FAIL
```
LARGER() means "larger is better", SMALLER() means "smaller is better"
PASS and FAIL mean the metric is pass fail, and the eval passed or failed.
