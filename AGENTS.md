# AGENTS.md
**Turfrace** is a fast-paced local multiplayer territory game for 2–8 players, built with the Bevy rust engine and playable in the browser, served as a static asset.

- Keep changes and aligned with the existing architecture; broad refactors are welcome if you identify code smells or if they significantly improve the code quality or architecture.
- Add or improve focused tests for every behavior change, bug fix, and boundary/security assumption, except for simple things such as constants.
- Keep the codebase structured, maintainable, and extendable. It should be easy to add much more functionality over time, such as new gamemodes and features.
- For agent changes, run `./scripts/feedback` before handing them in. It runs formatting, compilation, Clippy, and all tests, then reports only each step's duration and the total when successful. On failure it stops at the first failing step and prints a short diagnostic tail.
- Use `./scripts/feedback --quick` for intermediary checks when tests are slow; it runs formatting, compilation, and Clippy without tests.
