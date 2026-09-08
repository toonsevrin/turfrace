# Turfrace performance audit

Date: 2026-07-24; extended playtest pass: 2026-07-23–24; browser validation: 2026-09-08
Scope: authoritative simulation, startup, dynamic geometry, WebAssembly/WebGL2, NPCs, HUD, and deployment.

## Executive summary

The previous implementation fixed several render hot paths but retained a correctness and
performance trap: a sampled ownership grid was still the authority, then converted back into
smoothed contours for presentation. That made territory look blurred and made exact captures
dependent on cell resolution.

The refactor now uses deterministic fixed-point vector multipolygons as the sole ownership
authority. `i_overlay` performs union/difference/intersection at capture time; a small uniform
AABB index accelerates point ownership queries; the old board grid is rebuilt only as a derived
sample cache for broadphase and compatibility. Rendering consumes exact outer contours and holes
and triangulates them once per geometry revision, with an elevated top and explicit walls.

Gameplay presentation uses the custom `FlatMaterial` rather than `StandardMaterial`; no gameplay
light entities are spawned. Bevy's `3d` feature set still includes its PBR/cluster infrastructure,
so software fallback logs about CPU clustering and preprocessing are expected. Historical Chrome
SwiftShader measurements showed a cold MatchLoading block falling from 4.04 seconds to 0.73 seconds,
but those are not measurements of the current polish pass. Software frame timing is not
representative of physical GPU throughput.

## Implemented changes

### Territory and field

- Territory ownership is fixed-point vector multipolygon state. The sample grid is derived data and
  is never consulted for gameplay containment, capture, ranking, or elimination decisions.
- Boolean capture uses exact segment geometry and arena clipping. Loop captures fill the smaller
  valid boundary lobe; bridge captures claim only the stroked trail corridor.
- Exact outer contours and holes are triangulated directly. No Chaikin smoothing, raster contour
  extraction, owner-layer bias, or blurred edge coverage remains.
- Meshes are rebuilt only after capture, death, respawn, or a presentation-setting change; ordinary
  frames perform no territory CPU work or asset mutation.
- Owner meshes are bounded to the twelve competitor slots and use opaque materials, so there is no
  filtered alpha coverage or blurred territory edge in steady state.
- Earcut triangulation is isolated to this revision-time builder; there is no per-cell entity or
  per-frame clipping work.
- Cubes rise by the claimed-surface offset, preventing the black outline from being clipped by
  elevated territory. Trails query the same surface-height mapping.

### Shader and render cost

- A single lightweight `FlatMaterial` is used by gameplay instead of `StandardMaterial` variants.
- Flat shading uses one uniform color and a fixed directional normal term; shadows and translucent
  trails use the same shader with premultiplied alpha.
- No ambient or directional gameplay light entities are spawned. Bevy's PBR/cluster engine
  features remain enabled by the `3d` dependency preset, so this is not an engine-feature removal.
- Territory steady-state shading is one opaque material pass with a compact pattern function.
- Trail meshes remain persistent, update only on committed samples, and have a 256-point default
  render budget (128 on Low quality, 384 on High) with continuous joins and round caps.

### Simulation and allocation

- Trails retain an exact head while sampling bounded history for raster/collision/render work.
- Trail rasterization is incremental and collision uses per-cell segment buckets grouped by owner.
- A uniform territory AABB index narrows exact polygon containment queries. The sample grid keeps
  per-owner indexes only as a derived broadphase cache; death, seed, and capture claims refresh
  only the changed geometry AABB instead of rescanning the full board.
- Board generation uses exact radial-sector classification instead of testing every contour edge
  for every cell.
- NPC perception no longer clones every active trail polyline. It builds compact nearest-point
  perceptions only when an NPC think batch is due.
- NPC brains consume a compact typed local frame rather than global board queries. Capture planning
  samples the incrementally maintained owner-frontier mask within perception range, and trail
  lookup uses only nearby segment buckets.
- Tactical decisions and four-waypoint capture plans use fixed-size controller memory. Think timers
  are staggered and sensing/planning scratch buffers retain allocations after warm-up.
- Match-wide difficulty changes skill distributions and reaction quality only. It does not alter
  authoritative speed, turning, capture, collision, protection, or visibility rules.

### Extended playtest pass

- Territory no longer privileges a spawn anchor: captures remove only the geometry they cover, so
  disconnected owned islands remain valid and occupied islands do not cause displacement deaths.
  Loop closure now requires both trail endpoints to touch the same owned island; a trail joining two
  islands falls back to claiming only its corridor. This keeps the vector map authoritative while
  preventing a remote-lobe capture caused by nearest-contour snapping.
- Capture and death cache invalidation is AABB-bounded, collision-intent destinations and body
  snapshots reuse capacity, pending event vectors retain their allocations between fixed updates,
  and presentation camera/input/effect reconciliation avoids per-frame temporary hash
  collections. These changes target capture-time work and steady-state browser GC; no new FPS
  measurement is claimed here.
- The 64×64 exact-containment index now stores one contiguous 16-bit owner mask per cell instead of
  4,096 heap-backed vectors. Known-empty turf returns immediately rather than falling back to all
  twelve exact polygons, and index rebuilds become a single contiguous clear plus bit fills.
- Live closure resolution builds speculative claim geometry without also measuring a union and
  every opponent intersection that the commit step immediately repeats. Single captures also skip
  empty arbitration difference/union operations; simultaneous captures build only the committed
  geometry needed by a later contender.
- Fixed-tick ranking, respawn candidate, and NPC perception buffers now retain their small
  capacities between updates, eliminating recurring eight-player vector churn while preserving
  deterministic ordering and ranking-change events.
- Equal-time capture resolution also reuses its vector-result scratch buffer, so simultaneous
  closures do not repeatedly allocate a result list during the capture-time FPS spike.
- Effects use quality-dependent particle budgets and bounded lifetimes; captures get a brief
  expanding ring pulse. Procedural harmonic cues plus a sparse in-match ambient bed make menu,
  capture, kill, respawn, and victory states distinct without streamed audio assets.
- Menu backgrounds use animated, translucent turf marks and focus uses a narrow coral key,
  colored type, and a short scale transition rather than opaque rectangles, rounded controls, or
  underline rules. Settled controls stop writing UI style components until focus changes again.
- The arena now renders a dedicated warm paper edge beneath the bright top surface, giving
  oblique cameras a readable shallow slab profile. Live HUD copy is reduced to player identity,
  a compact color-coded leaderboard, and event announcements; dark diagnostic panels, duplicate
  rank/percentage readouts, and numeric speed readouts are gone, with color/tint carrying state.
- The top-right leaderboard is now three independent player-color rows with square accent rails;
  it omits territory percentage and redundant titling. `scripts/scoreboard-preview` captures a
  tight 280×110 crop for fast visual iteration without a panel mockup.
- Low and Medium quality use `Msaa::Off`; High uses `Msaa::Sample4` for single-player gameplay.
  Split-screen gameplay overrides that setting and uses `Msaa::Sample4` for every local camera.
  Each player `Camera3d` clears its own viewport to `SKY_COLOR`; there is no additional full-window
  clear camera. This preserves the current split-view attachment policy; its software cost has not
  been accepted as a physical-device performance result.
- Stable camera projection, viewport snapshots, crown/shield visibility, player-name color, and
  expired shake/pulse state use compare-before-write or idle early-outs instead of dirtying ECS
  components every rendered frame.
- Live HUD ranking, announcement, respawn, and elimination buffers retain their text capacity;
  unchanged colors and labels also skip component writes, keeping browser garbage collection out
  of the fixed-tick match loop.

### Lifecycle, respawn, and HUD

- The duplicate lobby countdown was removed. MatchLoading owns preparation; the canonical 3-2-1
  countdown starts only after the scene and pipelines have rendered preparation frames.
- Respawn resets `LastOwnedCell` to the new seed, so the next trail begins at the respawn territory
  instead of stale pre-death history.
- Gameplay HUD formatting is throttled to 10 Hz, changes text only when content differs, and uses
  compact status rails and floating identity/status text rather than opaque backing cards. The
  respawn countdown uses a lower placement that does not cover the cube and is hidden by a separate
  local-life system when alive.
- `scripts/build-web` builds with Cargo's size-oriented release profile (`opt-level = "s"`, thin
  LTO, one codegen unit), rejects debug-sized WASM, and reports compressed artifact size.

## Thermo-nuclear review findings

### Resolved blockers

1. **Grid-authoritative ownership:** raster cells could produce visible steps, blurred smoothing,
  and inconsistent collision decisions. Fixed-point multipolygons now own all territory state.
2. **Global territory work:** the renderer mixed snapshot invalidation, contour extraction, boolean
  clipping, triangulation, mesh lifecycle, and tests. It now receives exact vector contours and
  performs only bounded revision-time triangulation.
3. **Cold gameplay material cost:** gameplay uses a purpose-built flat material with prepass and
   shadows disabled instead of StandardMaterial; Bevy's broader `3d`/PBR engine support remains in
   the dependency set for other rendering paths.
4. **Trail lifecycle:** respawn retained a stale trail anchor. The anchor, current position, previous
   position, and heading are reset atomically and covered by a focused test.
5. **NPC snapshot churn:** every think batch cloned full trail vectors. Per-viewer nearest points are
   now computed without copying history.
6. **HUD churn:** strings and UI assets were rewritten every rendered frame. Projection now runs at
   10 Hz and performs compare-before-write updates.

### Residual risks

- A very large loop closure still allocates boolean intermediate contours. This is bursty
  capture-time work rather than startup or steady-state work; reusable overlay buffers and a
  segment-level spatial index are the next justified optimization if physical profiles show p95
  spikes.
- Independent split-screen cameras repeat scene traversal and draw submission even though their
  pixels partition the window. Physical 1/2/4/8-player profiles should set effect and visibility
  budgets before reducing visual clarity.
- Revision-time boolean normalization and triangulation are intentionally paid on ownership
  changes. They are bounded to owner meshes; capture-heavy mobile traces should verify their p95
  cost.

## Measurements

- Existing release artifact served from `dist/`: 39,468,216 bytes raw and 9,104,142 bytes gzip
  (built by `scripts/build-web`; reused for this validation, not rebuilt).
- Browser runtime validation used Chromium 151 at 1280×720, WebGL2 through ANGLE SwiftShader
  (`device_type: Cpu`). Home, two-player lobby, countdown, and two-player gameplay rendered;
  screenshots are under `target/browser-validation/`.
- During the post-ready browser sample, 67 animation frames were observed: median interval 66.6 ms,
  p95 533.4 ms, maximum 1,516.6 ms. This is a software-renderer observation with headless timing
  stalls, not evidence of hardware performance or a proven regression fix.
- No page errors, console errors, or failed requests occurred. Chromium emitted only expected
  SwiftShader capability fallbacks and AudioContext gesture warnings (plus the preload SRI warning).
- Historical Chrome SwiftShader figures remain historical only; no current FPS improvement is claimed
  and no pre-change regressed artifact was available for an apples-to-apples comparison.

## Acceptance criteria

- Countdown begins only after field, cameras, territory, competitors, and transparent materials
  have completed preparation frames.
- No full historical trail rasterization or collision scan occurs in a fixed update.
- Render trail history and territory geometry remain bounded.
- Trail-bit changes never invalidate territory presentation.
- Ownership changes invoke only fixed-point booleans, bounded vector triangulation, and mesh swaps.
- No full-board scan occurs for gameplay containment or capture decisions; the sample cache is
  refreshed only over the changed capture/death/seed AABB after a committed geometry mutation.
- Release deployment rejects debug artifacts and records raw/gzip size.
- Physical-hardware gates should target p95 fixed-update and presentation CPU below 8 ms for eight
  competitors at the agreed viewport configuration.
