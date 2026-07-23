# AGENTS.md
**Turfrace** is a fast-paced local multiplayer territory game for 2–8 players, built with the Bevy rust engine and playable in the browser, served as a static asset.

- Keep changes and aligned with the existing architecture; broad refactors are welcome if you identify code smells or if they significantly improve the code quality or architecture.
- Add or improve focused tests for every behavior change, bug fix, and boundary/security assumption, except for simple things such as constants.
- Keep the codebase structured, maintainable, and extendable. It should be easy to add much more functionality over time, such as new gamemodes and features.
- For agent changes, run `./scripts/feedback` before handing them in. It runs formatting, compilation, Clippy, and all tests, then reports only each step's duration and the total when successful. On failure it stops at the first failing step and prints a short diagnostic tail.
- Use `./scripts/feedback --quick` for intermediary checks when tests are slow; it runs formatting, compilation, and Clippy without tests.

## Resource limits
- Keep Cargo work bounded on the shared node: use `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, and focused test filters while iterating.
- Do not start concurrent Cargo builds/tests or leave an unbounded full-suite process running; use a timeout and stop it when diagnostics stall.
- Before handoff, run the required feedback script once with the bounded job count.
