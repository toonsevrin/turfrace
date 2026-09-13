# Diagnose sustained gameplay performance

Use **release** builds. A successful WASM compilation or headless test does not establish browser FPS, GPU stability, or memory safety. Measure early and late gameplay, not just the menu. The 60 Hz pacing budget is 16.67 ms per delivered frame; the playable floor is 33.33 ms (30 Hz). A good average with 100 ms spikes is not smooth.

## Browser: opt-in capture without DevTools

The server never builds, installs, launches a browser, or changes gameplay. It serves an existing bundle and refuses to overwrite an existing log. Normal mode remains manual:

```sh
./scripts/web-performance --dist dist --port 8080 \
  --output target/performance/web-run-01.jsonl
```

For a forwarded browser, explicitly opt into both the proxy origin and the page-side autostart. This is the command for the VS Code forwarding origin used by Turfrace:

```sh
./scripts/web-performance --dist dist --bind 0.0.0.0 --port 8080 \
  --allowed-origin https://turfrace.localc --auto-capture \
  --output target/performance/web-run-01.jsonl
```

Open `https://turfrace.localc/proxy/8080/?performance`. For direct local use, pass only `--auto-capture` and open `http://127.0.0.1:8080/?performance`. The `?performance` query is required; without it the index is served unchanged. Autostart waits until the read-only `window.__turfraceDiagnostics` bridge reports `app_state: 'Playing'`, `purpose: 'Playable'`, `phase: 'Running'`, and at least one human, then starts the existing 180-second probe. A small noninteractive overlay reports `waiting`, `recording`, `completed`, or `error`. It sends no input and performs no gameplay writes.

The probe is loaded through relative module URLs and posts to `./__performance/record` relative to the document base, so the same capture works below `/proxy/8080/`. Every five-second window includes a bounded ECS diagnostics snapshot (`purpose`, `phase`, elapsed seconds, and human/NPC counts). Use those snapshots to exclude menu, paused, or otherwise non-Running windows from performance conclusions; the roster label is the actual bridge roster at capture start. The explicit allowlist accepts the canonical loopback origin and the configured origin only. No wildcard or reflected CORS is enabled.

For manual capture in normal mode, open the loopback URL and use the browser console:

```js
import('./__performance/probe.mjs').then(p => p.start({
  seconds: 180, windowSeconds: 5, label: 'normal-8-players-1-human-view-1280x720'
}))
```

Each five-second window is posted to the local server and flushed to disk. Earlier windows survive a tab crash. The first rAF callback establishes the clock and is intentionally excluded from frame intervals, so startup delay is not misreported as a dropped frame. The probe records:

- rAF delivery mean/p95/p99/max, effective FPS, and frame counts over the 16.67 ms (60 Hz), 33.33 ms (30 Hz), and 100 ms thresholds, separately per window;
- `within60fpsRatio`/`within30fpsRatio`, strict `allFramesMeet60fps`/`allFramesMeet30fps`, and p95 pacing checks; these are not acceptance claims;
- browser, viewport, pixel ratio, elapsed time, and visibility changes;
- main-thread Long Tasks API count/total/max when the browser supports it;
- uncaught JS errors, unhandled rejections, and WebGL context loss;
- JS heap where exposed; **WASM/GPU memory are explicitly unavailable**, not zero.

Only windows with `hidden: false` and `visibilityChanges: 0` are eligible for a sustained-FPS claim. `effectiveFps` is derived from the mean interval and can hide spikes; inspect p95, p99, max, and the threshold counts as well. rAF intervals indicate callback pacing, not completed GPU work or compositor presentation.

The trace is bounded by duration/window limits; error/visibility events stop after 100 with an explicit omission record. A backed-up server drops ordinary records after 16 pending requests and reports the count; two reserved terminal slots preserve the final partial window and completion attempt. A missing `complete` record means an interrupted/unresponsive run, not proof of a crash cause. rAF intervals are **not** GPU execution times. Timer callbacks themselves cannot run while the main thread is blocked.

### Read a run and profile it

A compact view of the persisted window results (without installing another tool) is:

```sh
python3 - target/performance/web-run-01.jsonl <<'PY'
import json, sys
for line in open(sys.argv[1]):
    row = json.loads(line)
    if row.get('type') == 'window':
        print(f"{row['startMs']/1000:6.1f}-{row['endMs']/1000:6.1f}s "
              f"frames={row['frames']:4d} p95={row['p95Ms']:6.2f}ms "
              f"max={row['maxMs']:7.2f}ms 60fps={row['p95Meets60fps']} "
              f"30fps={row['p95Meets30fps']} hidden={row['hidden']} "
              f"long_tasks={row['longTaskCount']}")
PY
```

The final `complete` record includes `unsampledTailMs`: time after the last observed rAF. A large tail can hide a stall when the stop timer runs before rAF; do not treat such a capture as a clean smoothness pass. Buffered long tasks from before capture start are excluded.

For a 60 Hz run, investigate any eligible window whose p95 exceeds 16.67 ms, whose p99/max shows spikes, or whose `over16_67ms` count grows with match age. For the minimum playable tier, inspect p95/max, `allFramesMeet30fps`, and `over33_33ms` against the 33.33 ms reference. Do not turn these fields into a pass/fail result without recording device, refresh rate, browser, renderer, quality, and scenario. A 120 Hz display may deliver intervals below 16.67 ms, but that does not make a 60 Hz claim stronger; compare the same physical display configuration across builds.

Capture a clean timing run first, then a separate DevTools recording over a slow interval:

1. Open DevTools **Performance**, enable screenshots only if useful, and record 20–30 seconds around a reproducible slow section. Inspect Main/Long tasks, scripting, rendering, painting/compositing, GC, and the frame track.
2. Keep the probe running but do not merge profiler-overhead timings with the clean run. Save the DevTools trace beside the JSONL file and preserve console errors/context-loss messages.
3. Use the browser's `chrome://gpu` (or equivalent) to record WebGL renderer and hardware acceleration. Shift+Esc/Task Manager is useful for a coarse process-memory trend, but is not a WASM/GPU allocation measurement.

### Reproduction matrix and profiler

1. Record browser/version, OS/device/GPU, hardware acceleration, release commit/build, player/NPC count, difficulty, viewport count and canvas dimensions. The label is operator-supplied, not verified scenario metadata. Record seeds if available; otherwise mark unknown. This manual browser run is **not an automatically seeded replay**.
2. Compare a menu/control run, then 2, 4, and 8 human viewports (the native harness also uses a fixed twelve-NPC workload); use the same settings between builds. Capture at least three minutes of active play, including captures, deaths, and long trails. Repeat to distinguish noise from regressions.
3. Take a separate **DevTools Performance** recording as described above. Inspect main-thread WASM work, long tasks, rendering/compositing and GC. Save the trace with the JSONL log; profiler overhead means these timings should not be merged with the clean timing run.
4. Preserve console messages. For crashes, record browser crash/GPU diagnostics and OS memory pressure. JS heap alone cannot diagnose WASM or GPU exhaustion. A context-loss event is not necessarily an OOM.
5. Compare hardware acceleration enabled/disabled only as separate diagnostics; software-rendered/headless browser numbers do not represent a real GPU.

The probe needs browser ES modules and `fetch`. It uses `AbortSignal.timeout` or `AbortController` when available and retains a timer fallback so a dead recorder cannot block shutdown forever (the oldest fallback request may continue in the browser). Long-task counts depend on the Long Tasks API and are explicitly marked unavailable when unsupported. No Playwright/Chromium install is required. Automated browser launch, deterministic menu input, GPU timing, and physical-device validation are not implemented here.

## Native CPU: isolate simulation from rendering

```sh
mkdir -p target/performance
CARGO_BUILD_JOBS=2 timeout 900 cargo build --release --no-default-features \
  --example simulation_performance
# One minute at the game's fixed tick rate, streamed in five-second windows:
timeout 120 target/release/examples/simulation_performance \
  --ticks 3600 --window 300 --npcs 8 --seed 42 \
  > target/performance/simulation.jsonl
# Five-minute soak (timeout is a wall-clock limit, not simulated duration):
timeout 180 target/release/examples/simulation_performance \
  --ticks 18000 --window 600 --npcs 8 --seed 42 \
  > target/performance/simulation-soak.jsonl
```

Use `--npcs 12` as well to cover the current maximum competitor field; the CLI derives its limit from `MAX_COMPETITORS`.

This uses `HeadlessMatch::step`, including authoritative snapshot/fingerprint cost. It measures fixed simulation ticks, **not rendered FPS**. CLI bounds cap ticks, sample-window storage and roster size. Logs stream per window even if an external timeout terminates a run. Window one includes the cold gameplay start; do not silently discard it. Record whether the match has ended before interpreting a long idle tail.

For rendered native diagnostics use [performance-benchmark.md](performance-benchmark.md), including both `idle` and `capture-heavy`, at multiple viewport counts. Native software Vulkan/llvmpipe timings cannot establish GPU performance on the user's machine.

## Historical evidence from the earlier NPC performance repair

On this shared aarch64 node, release seed 42 / eight NPCs:

- Before optimization, the first 2,700 ticks averaged **43.76 ms/tick**, with a **2,617.61 ms** maximum. A 3,600-tick run timed out at 120 wall-clock seconds.
- Adding a conservative swept-bounds rejection to the canonical trail collision query removed repeated 20-iteration distance searches against far-away synthetic trail segments. Narrow-phase impact calculations remain unchanged. A 29,160-case equivalence regression covers scales, translation, degenerate segments, touching and crossing.
- Redundant safe-return forecasts and identical capture forecasts were removed; invalid routes no longer pay for threat scoring. Rival motion predictions are reused only within one planning call. Raster-bucket results are reused only while their exact bounds and immutable board agree; duplicate sorting is removed. The canonical bucket visitor filters to a bounded, sorted owner-specific selection before allocating. Raw bucket traversal and full active-trail cloning still scale with history. Candidate pruning uses an optimistic clearance bound and the unchanged ranking comparator. Polygon boundary distance now takes one square root after finding the minimum squared distance. No route safety or encounter acceptance threshold was relaxed.
- The final build's matched first 2,700 ticks averaged **2.79 ms/tick**, with a **90.10 ms** maximum (~16x mean improvement). These are CPU diagnostics, not browser FPS.
- The final eight-NPC five-minute simulation soak **hit its 180-second wall timeout**, with complete windows through **17,700 of 18,000 ticks**. Those windows averaged **10.03 ms/tick**, maximum **660.99 ms**; the last window averaged **11.98 ms**, p95 **67.70 ms**. Peak child RSS was ~12 MiB. This exposes remaining late-match CPU growth and severe spikes; it does not reproduce or exclude browser/WASM/GPU memory failure.
- Twelve-NPC one-minute runs improved from **6.90 to 5.63 ms/tick** after the later caching/pruning/geometry changes; maximum fell from **195.03 to 178.87 ms**. This is still far outside a consistent 60-Hz frame budget.
- All five saved pre-refactor encounter artifacts replay-verified on the final release build. Full `./scripts/feedback`, WASM compilation and diagnostic script tests passed. Timings and replay logs are in `target/performance/` (local, not versioned).

**Not a smoothness sign-off:** remaining long-frame spikes and sustained-growth behavior need further profiling. No browser or physical-GPU run was available here; the reported web crash has not been reproduced directly.

## Follow-up performance pass

Matched release, seed 42, shared aarch64 CPU measurements (all-NPC headless matches, not the browser's one-human roster):

| NPCs / ticks | Mean before → after | Worst window p95 before → after | Maximum tick before → after | Ticks >16.67 ms before → after |
| --- | --- | --- | --- | --- |
| 8 / 18,000 | 9.29 → 2.57 ms | 108.63 → 34.07 ms | 677.46 → 186.83 ms | 2,416 → 806 |
| 12 / 10,800 | 6.66 → 2.14 ms | 59.74 → 16.83 ms | 333.38 → 98.38 ms | 1,539 → 219 |

The extended 12-NPC 18,000-tick run averaged 2.56 ms, worst window p95 25.01 ms, maximum 98.38 ms. Window p95 maxima are **not global percentiles**. Shared-node maxima vary between runs. Raw local logs: `target/performance/current-baseline-{8,12}.jsonl` and `target/performance/verified-{8,12}-npcs.jsonl`; the 12-NPC comparison uses its first 18 windows. Maximum active-trail lengths match the baseline in every corresponding window, although that metric alone is not a full replay-equivalence proof.

Changes preserve exact geometry and planning candidate budgets: borrowed speculative trail prefixes; per-decision rival-return/threat-motion caches; ordered sparse frontier indexes with logarithmic updates; conservative arena margin rejection and exact nearest-edge shortlists; exact containment edge rejection. Application release code now uses speed optimization (`opt-level=3`), with dependencies retaining size optimization. No live trail truncation, geometry simplification, think-rate reduction, or weaker safety thresholds were introduced by this pass.

Use [simulation-performance.md](simulation-performance.md) for the new bounded, serial benchmark wrapper. `--phase-timing` instruments explicitly ordered simulation phases in a separate diagnostic match and reports snapshot overhead separately; it adds overhead and is not a rendered-frame measurement.

Validation: full `./scripts/feedback`, WASM compilation, and JS/Python diagnostic tests passed. Fresh 1,800-tick lab runs passed bait-disengagement, distracted-territory, interception, and return-race acceptance. The earlier loop-capture failure used the baseline personality for 1,800 ticks, not the authored builder/600-tick acceptance scenario; its bounded decision ring also displaced the initial maneuver. Re-running the documented builder scenario passes: first positive loop closure at tick 134, five captures and zero deaths in 600 ticks, with replay verification (`target/performance/validated-builder-loop.json`). The baseline run's deaths remain real, but are not evidence that the authored fixture fails. Passing seeded fixtures is not a blanket bot-behavior sign-off. Older saved repair artifacts could not replay because their schema lacks `tactic_sequence`; no replay-equivalence claim is made for them.

A subsequent exact Y-bucket containment index, batched frontier updates, sparse owner clearing, and conservative swept-collision rejection reduced the 8-NPC mean further to **1.21 ms**, maximum **39.32 ms**, and 113 over-budget ticks over 18,000 ticks (`target/performance/remaining-clean-8.jsonl`). In the accompanying separate phase run, capture/death/respawn maxima were 12.54/1.08/4.12 ms; NPC thinking remains the dominant tail. Return branch-and-bound now skips only candidates mathematically unable to beat the incumbent. Subsequent monotone 16-segment forecast-trail bounds avoid distant collision calls; fixed per-owner buckets preserve exactly the same spatial-reference selection without repeated full-output scans; forecasts with no old indexed history skip the otherwise empty query.

Latest seed-42 **36,000-tick** runs (ten simulated minutes):

| NPCs | Mean | Worst window p95 | Maximum tick | Ticks >16.67 ms |
| --- | --- | --- | --- | --- |
| 8 | 0.96 ms | 7.02 ms | 26.81 ms | 16 |
| 12 | 1.36 ms | 7.64 ms | 30.35 ms | 25 |

Logs: `target/performance/final-{8,12}.jsonl`. Every window's maximum active-trail length matches the preceding implementation; this is a useful continuity check, not a full replay-equivalence proof. Separate multi-seed/long-window stress runs recorded occasional shared-node wall-time outliers up to 50.91 ms, so these latest maxima are not universal guarantees. Full feedback, focused differential collision/ranking/selection tests, release WASM compilation, and all 15 Node/19 Python performance-tool tests pass.

Real Chromium/WASM runs at **1920×1080** completed three minutes with verified one-human-plus-7/11-NPC rosters, visible tabs, and approximately real-time simulation. SwiftShader delivered only 14.48/13.14 FPS; these are software-rendering results, not representative laptop-GPU measurements. Evidence: `target/browser-performance/chunked-7-1080p/` and `target/browser-performance/final-auto-11-1080p/`. The latter also verified the click-only recorder end-to-end: 36 windows plus start/completion, zero dropped records, actual GPU-renderer identification and WASM filename, and a unique capture ID on every record. Profiles remain separate from clean timing.

**Hardware evidence is partial:** remote Firefox/M1 capture `2f86484c-b067-4f55-afa7-5174050430b8` verified one human plus 7 bots on build `148fa303cc1161bb`. It uploaded 35 five-second windows (no completion record), mostly near 60 rAF deliveries/s, with one 50 ms interval around 145–150 seconds. The last window includes GameOver. The user still reports occasional whole-scene FPS drops; there is no 11-bot hardware capture yet. This is not a sustained-60 or minimum-30 sign-off, nor a GPU presentation-time measurement. Local browser rendering remains SwiftShader-only; physical controllers remain unverified.

### Main-app stall attribution

With `?performance`, the browser now publishes bounded cumulative `frame_timing` diagnostics per playable match generation, at approximately 5 Hz (also flushing the final measured frame). Timing hooks span `First` to `Last`, and each `FixedFirst` to `FixedLast`. They report sums, counts over the frame budgets, maximum catch-up tick count, and a paired slowest frame with its frame number, fixed duration/count, longest fixed tick, and non-fixed remainder. No per-frame history is retained. New matches reset the counters; GameOver retains the summary.

These are **main-app wall durations**, not CPU clock or GPU timings. The remainder includes all measured non-fixed work, not just rendering. Async render extraction/sub-app work, GPU execution and presentation outside these hooks are not measured. The recorder's rAF measurements remain separate; nested values are cumulative, not five-second window totals. Ordinary play without the query installs no timing hooks.

End-to-end upload verified in `target/browser-performance/frame-timing-upload/`, build `8d047dd4d149c075`: a local software-rendered 11-bot sample recorded a 45 ms main-app frame containing 43.5 ms of fixed work over three ticks (longest tick 34.8 ms). This validates attribution plumbing, not the cause of the user's hardware-browser hitches.

## Diagnostic script tests

```sh
node --test scripts/web-performance-probe.test.mjs
python3 scripts/web_performance_test.py
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 timeout 120 cargo test \
  --no-default-features --example simulation_performance
```
