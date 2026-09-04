

# kiss test
- Ideally, `kiss test` would be constantly working, use all of the CPUs it was allocated (via num_jobs), show a fairly steady stream of meaningful logging output (so the user knows it's working), and be very efficient with resources.
- Also, the user should be able to kill it at any time with CTRL-C and restart with minimal repeating of work. DON'T try to do cleanup at exit time, though. Just exit on CTRL-C quickly. Defer any housekeeping that might be necessary until the next time 'kiss test' is started.



