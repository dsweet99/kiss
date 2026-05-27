
# Goal
file_rmse < 0.2 on
- `./ops/coverage_discrepancy.py rust ../repos/ripgrep/`
- `./ops/coverage_discrepancy.py rust ../repos/ruff/`
- `./ops/coverage_discrepancy.py rust ../malvin/`
- `./ops/coverage_discrepancy.py rust ../ennbo`
- `./ops/coverage_discrepancy.py python ../repos/rich`
- `./ops/coverage_discrepancy.py python ../repos/rope`
- `./ops/coverage_discrepancy.py python ../repos/ruff`
- `./ops/coverage_discrepancy.py python ../ennbo`
- `./ops/coverage_discrepancy.py python ../yubo`

See `./ops/coverage_discrepancy.py --help` for how to get more detailed information. The details will probably be helpful.

# Constraints
- kiss must stay fast.
- kiss must not execute the code being measured.

# Resources
- slipcover and cargo-llvm-cov are both installed. Consider their measurements the gold standard.
- There are several Rust and Python repos in /Users/dsweet2/Projects/repos/ that you can use for coverage measurement.
- Feel free to browse the web.
- Feel free to git clone other test repos into /Users/dsweet2/Projects/repos/.
- 4GB of memory. You'll need to make your tests, evaluations, etc. work within that memory, or you'll get OOM killed.

# Ideas
- A good idea might be to measure coverage test by test.
- The file ideas.md has some ideas. #1 looks good to me.

# Actions
- You may modify kiss's code, build it, test, and so on.
- You may write new code to do the measurements. Some of it might be one-off code. Some of it might keep code to keep and reuse in the future. (I'd like that. That way we can use it to keep improving kiss.)
- You may run code and take lots of measurements. This machine is your for the day.

