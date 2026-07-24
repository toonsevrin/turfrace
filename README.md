# Turfrace

Turfrace is a fast local multiplayer territory game for 2–8 humans, with NPCs filling a configurable 2–12 competitor field. Draw loops, steal turf, and cut exposed trails; the first competitor to control 95% of the irregular paper arena wins.

The v0.1 implementation is a static Rust/Bevy 0.19 WebAssembly application. It has no server, telemetry, or network play. Profiles, preferences, lobby choices, and lifetime statistics remain in browser-local storage.

Territory is exact fixed-point vector geometry with a bounded render/broadphase cache. Every
competitor has one spawn-anchored island: a capture severs any other lobe, and a cube standing on
that removed lobe is displaced through the normal respawn flow.

## Run it

Native development:

```sh
cargo run
```

Web development requires the WASM target and [Trunk-rs](https://github.com/trunk-rs/trunk):

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
trunk serve --open
```

To test the optimized browser build locally, let Trunk build the release WASM
and serve the generated static site:

```sh
trunk serve --release --open
```

This serves the same release output that is deployed, with live reload enabled.
For a deploy-style build followed by a separate static server, run:

```sh
./scripts/build-web
python3 -m http.server 8080 --directory dist
```

Create the optimized static site with:

```sh
./scripts/build-web
```

The helper enforces a release Trunk build, rejects debug-sized WASM, and records gzip (and Brotli when installed) sizes. The deployable files are written to `dist/`. Serve them from HTTPS for reliable browser Gamepad API access. WebGL2 is the baseline renderer; the custom paper, territory, and trail shaders do not require WebGPU.

## Controls

- Gamepad: either stick steers, A/Cross confirms or readies, B/Circle goes back, D-pad/left stick navigates, shoulders cycle lobby colors, and Start pauses.
- Mouse: click `JOIN WITH MOUSE`, use the player card's ready control, and steer toward the cursor inside that player's viewport.
- Keyboard: Enter/Space joins or toggles ready; WASD/arrow keys join when unassigned, then steer and navigate, and Escape pauses or goes back.

If a controller disconnects during play, the match pauses. An unassigned controller can reclaim the player, or the paused player can be replaced with an NPC.

## Quality checks

The repository's normal handoff check formats, compiles, lints, and runs every test:

```sh
./scripts/feedback
```

Use `./scripts/feedback --quick` while iterating. The test suite covers geometry, capture and combat ordering, respawn and victory boundaries, persistence/input/lobby behavior, viewport layouts, render mappings, and an accelerated deterministic one-hour NPC soak.

## Controlled visual playtests

The visual harness boots the real game plugins at a fixed timestep and captures deterministic user-facing frames:

```sh
./scripts/visual-feedback home
./scripts/visual-feedback leaderboard
./scripts/visual-feedback match --frames 180
./scripts/visual-feedback match --seconds 6
./scripts/visual-feedback capture --seconds 6
./scripts/visual-feedback results --width 960 --height 600
./scripts/visual-feedback settings
./scripts/visual-feedback --all
```

Images are written to `target/visual-feedback/`. Frame and second offsets use the game's fixed 60 Hz clock, while optional dimensions make layout stress tests repeatable. The script uses Xvfb and Mesa's software Vulkan driver by default so people or automated review agents can inspect actual rendering in headless Linux. `VK_ICD_FILENAMES`, `WGPU_BACKEND`, and `WGPU_SETTINGS_PRIO` remain overridable for another test environment.

For fast territory-only iteration, the predefined-shape review tool exercises the production smooth
vector-surface renderer without starting a match:

```sh
./scripts/territory-review target/visual-feedback/territory-review.png
```

## Architecture

`src/main.rs` only composes three top-level plugins:

- `AppShellPlugin`: states, profiles/persistence, input abstraction, lobby, responsive UI, and browser integration.
- `MatchPlugin`: the deterministic 60 Hz authoritative board, movement, trails, capture, combat, ranking, respawn, and NPC simulation.
- `PresentationPlugin`: split-screen cameras, revision-built elevated territory surfaces, lean flat-shaded cube/trail/effect rendering, and procedural shared audio.

Gameplay is 2D and deterministic even though presentation is 3D. Input and NPCs both produce the same steering intent, and presentation consumes simulation events without owning rules. Territory ownership is converted into bounded smooth owner meshes only when its revision changes; the grid is never rendered directly and no territory work runs on ordinary frames.
