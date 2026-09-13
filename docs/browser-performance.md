# Browser performance smoke capture

`browser-performance.mjs` measures an already-served Turfrace release bundle with the globally installed Playwright and SwiftShader Chrome. It never builds, installs, or starts a server.

```sh
node scripts/browser-performance.mjs \
  --url http://127.0.0.1:8080 --bots 7 --seconds 180 \
  --width 1920 --height 1080 \
  --output target/browser-performance/run-01
```

The harness appends `?performance=1` and requires the current WASM diagnostic bridge. The bridge is read-only and publishes `window.__turfraceDiagnostics` from ECS: the `AppState` debug value, `MatchSession` purpose/phase/elapsed time, and human/NPC counts from `Query<&Competitor>`. An older bundle without this opt-in bridge is a blocker, not an unverified success.

A fresh context configures the canvas-only lobby at its verified 1280×720 layout, claims one keyboard racer, and clicks the NPC plus control to the requested count. After launch it resizes the running match to the requested measurement viewport (default 1280×720; specify 1920×1080 for the performance target). Lobby pixel coordinates are not assumed to scale across responsive layouts. Every step is checked through `localStorage` key `turfrace.last_lobby.v1` (`npc_count`, including the default zero), not a screenshot or raster hash. After readying, it waits for `Playing`/`Playable`/`Running` with exactly one human and the requested NPC roster. It then polls the ECS diagnostics at 1 Hz; any pause, phase change, or roster change fails the clean run.

Evidence includes `metadata.json`, `timing.json`, `diagnostics.json`, `ecs-diagnostics.json`, `home.png`, `lobby-configured.png`, `before.png`, and `after.png`. Timing is requestAnimationFrame delivery: mean/p95/p99/max plus counts over 16.67, 33.33, and 100 ms. The capture records initial and final unsampled gaps and every visibility transition; zero frames, a hung rAF, hidden/transitioned pages, or a large final gap are blockers. ECS elapsed time is compared with wall time and reported separately as `timing.simulation`; a slowdown is not silently presented as an FPS result.

`--capture-only` skips menu automation but still requires a running playable match and the diagnostic bridge. `--cpu-profile FILE` performs a separate Chromium CPU profile run. Clean timing, metadata, browser diagnostics, and ECS observations are persisted before the optional profile starts; a profile failure is recorded in `cpu-profile-error.json` without discarding clean evidence. The installed inventory is SwiftShader/no GPU, so every result is labelled **software-rendered** and is explicitly **not hardware-FPS sign-off**.

The opt-in integration test runs both target rosters serially and verifies the actual gameplay screenshots are 1920×1080 (not just requested viewport metadata):

```sh
TURFRACE_BROWSER_INTEGRATION_URL=http://127.0.0.1:8081/ \
  timeout 190 node --test scripts/browser-performance.integration.test.mjs
```

It requires an already running release server and installs nothing. Without the environment variable it is skipped by the ordinary Node test sweep.
