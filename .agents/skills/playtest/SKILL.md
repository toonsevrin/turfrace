---
name: playtest
description: Run controlled Turfrace gameplay and visual QA using deterministic timed screenshots, viewport stress captures, native checks, WebAssembly compilation, and physical controller validation. Use when reviewing presentation quality, reproducing a user-visible frame, checking menus or split-screen gameplay, validating web/controller behavior, or preparing a release handoff.
---

# Playtest

Test the real game plugins and judge the rendered result, not merely compilation or unit tests.

## Establish a clean baseline

1. Read `AGENTS.md`, `SPEC.md`, and the controlled-playtest section of `README.md`.
2. Run `./scripts/feedback --quick` before visual work. Fix formatting, compilation, and Clippy failures before interpreting screenshots.
3. Keep the scenario seed, capture time, and viewport dimensions fixed while comparing a visual change.

## Capture deterministic frames

Use `scripts/visual-feedback`; it advances the real game at its fixed 60 Hz clock and writes PNGs under `target/visual-feedback/` by default.

```sh
./scripts/visual-feedback home
./scripts/visual-feedback lobby
./scripts/visual-feedback match --frames 30
./scripts/visual-feedback match --seconds 3
./scripts/visual-feedback match --seconds 6
./scripts/visual-feedback pause
./scripts/visual-feedback results --width 960 --height 600
./scripts/visual-feedback --all
```

Use `--output PATH.png` to preserve comparison frames. Use early and late match captures: spawn-only screenshots do not expose capture topology, respawn effects, trail turns, or accumulated HUD noise.

## Review the actual PNGs

Open every relevant screenshot at original resolution. Check:

- menu hierarchy, square outlined typography, contrast, focus state, concise copy, and absence of clipping;
- all eight result rows and actions at 1280×720, plus a 960×600 stress capture;
- split-screen boundaries, local HUD ownership, global leaderboard placement, and readable elimination/respawn text;
- continuous claimed surfaces with no raw cell staircase, cracks, white holes, overlaps, chunk seams, or arena-edge leakage;
- rounded trail caps and joins, stable opacity, animated shimmer, eased cube heading/lean, and effects that do not obscure gameplay;
- field silhouette, paper treatment, player/color-pattern readability, and sensible visual density during real play.

Treat any artifact visible in a deterministic capture as reproducible. Correct the cause, recapture the same frame, and compare again.

## Exercise interaction

For native manual play, run `cargo run`. Verify mouse joining/steering, keyboard navigation, pause/resume, and lobby focus order.

For browser play, run `trunk serve`, open the local page, and verify canvas resizing, fullscreen, local persistence, WebGL2 rendering, and audio startup. Test real controllers where available:

- press a button to expose and join each gamepad;
- steer with both sticks and navigate with D-pad/left stick;
- confirm/back, color cycling, ready state, and Start-to-pause;
- disconnect during play, then reclaim the slot or replace it with an NPC;
- verify two or more simultaneous controllers receive distinct split-screen views.

## Finish the pass

Run:

```sh
cargo check --target wasm32-unknown-unknown --lib --bin turfrace
./scripts/feedback
```

Report the scenarios, times, dimensions, and screenshot paths reviewed. Distinguish expected headless warnings (software Vulkan and absent audio hardware) from shader, allocator, panic, or gameplay errors, which must be fixed.
