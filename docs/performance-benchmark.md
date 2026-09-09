# Performance benchmark and physical validation

The benchmark uses the production plugins rather than a mock scene and runs through Bevy's real
winit runner. It measures native wall-clock intervals between `First` schedule passes,
deterministic fixed-update count, observed capture statistics, and viewport geometry/camera pixel
area. First-to-First intervals include event-loop work, render submission, and present waiting;
they are not CPU or GPU timings. It does **not** claim browser FPS, browser startup, or memory
results.

## Reproduce native runs

From the repository root:

```sh
./scripts/performance-benchmark --scenario idle
./scripts/performance-benchmark --scenario capture-heavy --frames 1200
```

The script builds the new `performance_benchmark` example once, then runs 1, 2, 4, and 8
viewports at 1920x1080 with 120 warm-up and 600 measured frames. Use `--no-build` when the
release example has already been built, and `--views 8 --frames 1200` for a focused run. The
output is `target/performance/benchmark.json` unless `--output` is supplied.

Both scenarios preserve the same twelve seeded Normal NPC controllers for every view count
(match seed `0x5eedcafe`, roster seed `0x5eedbeef`). The first
N `Competitor.kind` values are classified as local subjects only to expose N canonical cameras;
they still use `NpcController` and no `HumanController` or device adapter. `idle` sets
`player_speed` to zero, while `capture-heavy` uses normal speed and the same scene workload.
The latter reports `captures_observed` from production `MatchStatistics`; zero captures is
reported as `capture_activity_verified: false`, not replaced with a made-up workload.

The per-view input records include:

- startup-to-first-First and startup-to-generation-matched presentation readiness wall timings;
- median, p95, minimum, and maximum First-to-First wall interval plus its equivalent p95 FPS;
- fixed schedules observed during the measured window;
- competitor and controller counts (the twelve NPC controllers are constant; human controllers are
  always zero), the scenario's configured speed, and observed captures;
- every visible player camera's slot, subject position, physical viewport rectangle, and pixel
  area. Per-camera CPU/GPU timings are `null` because the production renderer does not expose
  them. Fixed-update CPU timing is also `null`; browser, physical-device, and memory verification
  flags remain false.

The aggregate measures an existing `dist/*.wasm` artifact's raw and gzip size (and Brotli when
installed). If there is no exactly-one WASM artifact, sizes are `not_measured`; this report makes
no size claim until that measurement is actually performed. Run `scripts/build-web` separately
when a fresh deploy artifact is required.

Native timing is useful for detecting a local change, but the default Xvfb/Mesa setup may use
software Vulkan. The report therefore marks browser, physical hardware, memory, and fixed-system
CPU budgets `unverified`. Do not compare the native wall-time p95 with the SPEC's 8 ms fixed CPU
gate as if they were the same measurement.

Tooling self-tests do not require Cargo:

```sh
python3 scripts/performance_budget_test.py
```

## Representative validation tiers

These are representative gates to run, not hardware claims about an untested machine.
Record the exact model, OS, browser version, WebGL renderer, viewport, device-pixel ratio,
quality, and whether power saving was enabled.

| Tier | Representative device | Browser configuration | Gate to inspect |
| --- | --- | --- | --- |
| A: desktop | recent discrete-GPU desktop or laptop | WebGL2, Medium, 1920x1080 | 60 FPS target for 1/2/4/8 views |
| B: integrated | recent integrated-GPU laptop | WebGL2, Medium, 1920x1080 | sustained 45 FPS minimum, especially 8 views |
| C: constrained | older integrated laptop or tablet-class device | WebGL2, Low, reduced motion if needed | playable 30 FPS minimum; document compromises |

The SPEC's 256 MB post-load memory target and 8 ms p95 fixed-update CPU target remain separate
measurements. A browser's task manager/devtools can approximate memory, but it must be labelled
as such. The benchmark's `fixed_update_cpu_ms` is intentionally null.

## Build feature boundary

The release/native benchmark build uses `bevy` with default features disabled and enables only the
`bevy_pbr`, UI, and audio features required by the shell. The former `bevy/3d` umbrella is not
enabled. This trims unrelated 3D facilities while retaining the PBR material pipeline required by
`FlatMaterial`; it does not make PBR or GPU cost disappear. No binary or WASM size reduction is
claimed here without a fresh artifact measurement.

## Repeatable physical browser pass

1. Build a release site with `scripts/build-web`, serve `dist/` over HTTPS, and use a clean browser
   profile or clear Turfrace local storage. Do not use `trunk serve` debug output.
2. Set the browser window to 1920x1080, record device-pixel ratio and renderer from
   `chrome://gpu` (or the browser equivalent), select Medium, and keep the same browser tab
   foregrounded. Disable unrelated tabs and battery-saving throttles.
3. Record cold startup from navigation until the first stable home frame using the browser's
   Navigation Timing/Performance panel. Repeat three cold loads and report all samples and the
   median; the native benchmark startup field is not a browser startup substitute.
4. Start a lobby and run separate 1, 2, 4, and 8-human passes. Use distinct real controllers,
   confirm each local HUD and viewport, and keep the same 30-second post-countdown window. For
   each pass, capture a 10-second settled trace after a 10-second warm-up. Record FPS/frame
   interval percentiles, long tasks, and any context-loss or shader errors.
5. Repeat the four passes with NPCs filling the field. Exercise steering long enough to produce
   several real loop captures and trail collisions; record the number of captures and deaths
   observed. If no capture occurs, extend the run and mark capture-heavy coverage incomplete.
6. In the browser Performance panel, inspect scripting/rendering separately from GPU raster time.
   Do not assign fixed-update CPU time from total frame time. If a browser profiler can isolate
   fixed updates, report its method and sampling overhead; otherwise leave that metric unverified.
7. Repeat the 8-view Medium pass on the integrated and constrained tiers, then repeat the
   constrained pass on Low. Save traces/screenshots outside the repository or link them from the
   report so the exact viewport and quality are auditable.

Controller validation should use the real browser Gamepad API: expose/join each pad, steer with
both sticks, pause with Start, disconnect one pad, reclaim or replace it, and verify two or more
simultaneous controllers retain distinct camera subjects. Headless screenshots and software
Vulkan logs are visual QA evidence only, not physical-browser performance evidence.
