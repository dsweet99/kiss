# Structure

Semantically, kiss is organized as a sequence of layers:

1. Raw execution and data substrate: basic capabilities for reading source trees, parsing files, representing findings, storing cached facts, and running language-specific test and coverage tools. Python execution uses the rslip coverage and result cache plus a pytest runner boundary; Rust test execution uses a cargo-nextest runner boundary.
2. Language understanding: knowledge of individual Python and Rust files, including functions, types, modules, imports, statements, and code units.
3. Structural interpretation: whole-codebase relationships such as dependency graphs, reachable modules, cycles, fan-out, and transitive dependencies.
4. Quality measurement: maintainability signals such as complexity, size, duplication, and rule violations.
5. Repository-level decision making: a shared ensure kernel, with language-neutral contracts and Python/Rust runtimes, decides whether an execution witness can be accepted, which selectors must be repaired, what must be published, which gates pass or fail, and which tests are relevant.
6. User-facing workflows: actions such as checking the repo, showing stats, visualizing dependencies, moving symbols, generating configuration, detecting duplicates, and running covering tests.
7. Command interface: the shell that accepts user intent, loads configuration, invokes the right workflow, prints results, and returns success or failure.
