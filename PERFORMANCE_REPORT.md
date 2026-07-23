# Turfrace performance audit

Date: 2026-07-22  
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

StandardMaterial was removed from gameplay presentation. Cubes, trails, borders, shadows, and
effects share a small directional-flat shader with lighting baked from mesh normals; real-time
lights and their clustering work were removed. Under identical Chrome SwiftShader WebGL2
conditions, the cold MatchLoading block fell from 4.04 seconds to 0.73 seconds and sustained
software frame intervals improved from roughly 370-400 ms to 250-270 ms. The remaining timing is
software rasterization and is not representative of physical GPU throughput.

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

- A single lightweight flat material replaces all gameplay StandardMaterial variants.
- Flat shading uses one uniform color and a fixed directional normal term; shadows and translucent
  trails use the same shader with premultiplied alpha.
- Ambient and directional light entities were removed because gameplay materials no longer need
  Bevy's lighting or shadow pipeline.
- Territory steady-state shading is one opaque material pass with a compact pattern function.
- Trail meshes remain persistent, update only on committed samples, and have a 512-point render
  budget with continuous joins and round caps.

### Simulation and allocation

- Trails retain an exact head while sampling bounded history for raster/collision/render work.
- Trail rasterization is incremental and collision uses per-cell segment buckets grouped by owner.
- A uniform territory AABB index narrows exact polygon containment queries. The sample grid keeps
  per-owner indexes only as a derived broadphase cache; death and seed claims update it once per
  committed vector change.
- Board generation uses exact radial-sector classification instead of testing every contour edge
  for every cell.
- NPC perception no longer clones every active trail polyline. It builds compact nearest-point
  perceptions only when an NPC think batch is due.
- NPCs avoid nearby competitors, opportunistically hunt exposed trails by personality, and use
  stable outward patrol steering.

### Lifecycle, respawn, and HUD

- The duplicate lobby countdown was removed. MatchLoading owns preparation; the canonical 3-2-1
  countdown starts only after the scene and pipelines have rendered preparation frames.
- Respawn resets `LastOwnedCell` to the new seed, so the next trail begins at the respawn territory
  instead of stale pre-death history.
- Gameplay HUD formatting is throttled to 10 Hz, changes text only when content differs, and uses
  compact status rails rather than large opaque rectangles.
- The release build script rejects debug-sized WASM and reports compressed artifact size.

## Thermo-nuclear review findings

### Resolved blockers

1. **Grid-authoritative ownership:** raster cells could produce visible steps, blurred smoothing,
  and inconsistent collision decisions. Fixed-point multipolygons now own all territory state.
2. **Global territory work:** the renderer mixed snapshot invalidation, contour extraction, boolean
  clipping, triangulation, mesh lifecycle, and tests. It now receives exact vector contours and
  performs only bounded revision-time triangulation.
3. **Cold PBR compilation:** unlit StandardMaterial still compiled the general PBR shader family.
   Gameplay now uses a purpose-built flat material with prepass and shadows disabled.
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

- Release WASM: 38,956,183 bytes raw and 8,941,290 bytes gzip.
- Identical 1280x720 Chrome SwiftShader trace, before flat-material conversion: 4.04-second cold
  driver block; subsequent intervals around 370-400 ms.
- Identical trace after conversion: 0.73-second largest cold block; subsequent intervals around
  250-270 ms; no second block after the countdown.
- Native deterministic screenshots were reviewed at 1280x720 for territory-only and two-player
  match scenarios. Browser gameplay was also captured at 1280x720 with no shader or runtime errors.
- SwiftShader and Mesa llvmpipe are CPU renderers. Their absolute frame rates are not physical-device
  acceptance data, but the same-environment delta and long-task placement are valid.

## Acceptance criteria

- Countdown begins only after field, cameras, territory, competitors, and transparent materials
  have completed preparation frames.
- No full historical trail rasterization or collision scan occurs in a fixed update.
- Render trail history and territory geometry remain bounded.
- Trail-bit changes never invalidate territory presentation.
- Ownership changes invoke only fixed-point booleans, bounded vector triangulation, and mesh swaps.
- No full-board scan occurs for gameplay containment or capture decisions; the sample cache is
  rebuilt once after a committed geometry mutation.
- Release deployment rejects debug artifacts and records raw/gzip size.
- Physical-hardware gates should target p95 fixed-update and presentation CPU below 8 ms for eight
  competitors at the agreed viewport configuration.
