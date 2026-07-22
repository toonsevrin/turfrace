# Turfrace

Turfrace is a fast local multiplayer territory game for 2–8 humans, with NPCs filling a configurable 2–12 competitor field. Draw loops, steal turf, and cut exposed trails; the first competitor to own the whole irregular paper arena wins.

The v0.1 implementation is a static Rust/Bevy 0.19 WebAssembly application. It has no server, telemetry, or network play. Profiles, preferences, lobby choices, and lifetime statistics remain in browser-local storage.

## Run it

Native development:

```sh
cargo run
```

Web development requires the WASM target and [Trunk](https://trunkrs.dev/):

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk
trunk serve --open
```

Create the optimized static site with:

```sh
trunk build --release
```

The deployable files are written to `dist/`. Serve them from HTTPS for reliable browser Gamepad API access. WebGL2 is the baseline renderer; the custom paper, territory, and trail shaders do not require WebGPU.

## Controls

- Gamepad: either stick steers, A/Cross confirms or readies, B/Circle goes back, D-pad/left stick navigates, shoulders cycle lobby colors, and Start pauses.
- Mouse: click to join and steer toward the cursor inside that player's viewport.
- Keyboard fallback: Enter/Space joins or confirms, WASD/arrow keys steer and navigate, and Escape pauses or goes back.

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
./scripts/visual-feedback match --frames 180
./scripts/visual-feedback match --seconds 6
./scripts/visual-feedback results --width 960 --height 600
./scripts/visual-feedback --all
```

Images are written to `target/visual-feedback/`. Frame and second offsets use the game's fixed 60 Hz clock, while optional dimensions make layout stress tests repeatable. The script uses Xvfb and Mesa's software Vulkan driver by default so people or automated review agents can inspect actual rendering in headless Linux. `VK_ICD_FILENAMES`, `WGPU_BACKEND`, and `WGPU_SETTINGS_PRIO` remain overridable for another test environment.

## Architecture

`src/main.rs` only composes three top-level plugins:

- `AppShellPlugin`: states, profiles/persistence, input abstraction, lobby, responsive UI, and browser integration.
- `MatchPlugin`: the deterministic 60 Hz authoritative board, movement, trails, capture, combat, ranking, respawn, and NPC simulation.
- `PresentationPlugin`: split-screen cameras, chunked territory and custom materials, cube/trail/effect rendering, and procedural shared audio.

Gameplay is 2D and deterministic even though presentation is 3D. Input and NPCs both produce the same steering intent, presentation consumes simulation events without owning rules, and territory rendering is revision-gated and chunked. This keeps new game modes, NPC brains, render treatments, and platform adapters independently extensible.
