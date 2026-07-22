# Turfrace performance audit

Date: 2026-07-22  
Scope: authoritative simulation, startup, dynamic geometry, WebAssembly/WebGL2, NPCs, HUD, and deployment.

## Executive summary

The pending implementation fixed several real hot paths but retained its largest architectural
problem: territory ownership was converted into global contours, clipped between owners,
triangulated, and uploaded as replacement meshes. A cold browser trace also reproduced a
4.04-second main-thread block while Bevy compiled full StandardMaterial/PBR variants for a flat
cartoon presentation.

The territory pipeline is now capture-time vector geometry: at most one smooth opaque mesh per
owner, with an elevated top and a shallow side wall. Ownership is converted to boundary loops,
smoothed, and triangulated only when the revision changes; ordinary frames submit the existing
meshes without CPU work or texture filtering. The old 1,700-line contour implementation was
replaced by a focused bounded builder.

StandardMaterial was removed from gameplay presentation. Cubes, trails, borders, shadows, and
effects share a small directional-flat shader with lighting baked from mesh normals; real-time
lights and their clustering work were removed. Under identical Chrome SwiftShader WebGL2
conditions, the cold MatchLoading block fell from 4.04 seconds to 0.73 seconds and sustained
software frame intervals improved from roughly 370-400 ms to 250-270 ms. The remaining timing is
software rasterization and is not representative of physical GPU throughput.

## Implemented changes

### Territory and field

- Territory ownership remains grid-authoritative but the grid is never rendered directly.
- Capture-time boundary extraction emits smooth owner surfaces and explicit side walls. The board
  grid remains authoritative but is never visible as a grid.
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
- Ownership maintains per-owner cell indexes; death and seed claims avoid unrelated board cells.
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

1. **Global territory work:** the renderer mixed snapshot invalidation, contour extraction, boolean
   clipping, triangulation, mesh lifecycle, and tests in one 1,700-line file. It is now a focused,
   bounded vector-surface module of roughly 450 lines.
2. **Misleading optimization:** affected-owner invalidation still triggered cross-owner clipping and
   large allocations. The entire contour architecture, not just its invalidation flags, was removed.
3. **Cold PBR compilation:** unlit StandardMaterial still compiled the general PBR shader family.
   Gameplay now uses a purpose-built flat material with prepass and shadows disabled.
4. **Trail lifecycle:** respawn retained a stale trail anchor. The anchor, current position, previous
   position, and heading are reset atomically and covered by a focused test.
5. **NPC snapshot churn:** every think batch cloned full trail vectors. Per-viewer nearest points are
   now computed without copying history.
6. **HUD churn:** strings and UI assets were rewritten every rendered frame. Projection now runs at
   10 Hz and performs compare-before-write updates.

### Residual risks

- A very large loop closure still allocates A* cost/parent arrays and synchronously scans its polygon
  bounds. This is bursty capture work rather than startup or steady-state work; generation-stamped
  scratch buffers are the next justified simulation optimization if physical profiles show spikes.
- Independent split-screen cameras repeat scene traversal and draw submission even though their
  pixels partition the window. Physical 1/2/4/8-player profiles should set effect and visibility
  budgets before reducing visual clarity.
- Revision-time contour smoothing and triangulation are intentionally paid on ownership changes.
  They are bounded to owner meshes; capture-heavy mobile traces should verify their p95 cost.

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
- Ownership changes invoke only bounded contour extraction, smoothing, triangulation, and mesh swaps.
- No full-board scan occurs on death, seed claim, or respawn-candidate construction.
- Release deployment rejects debug artifacts and records raw/gzip size.
- Physical-hardware gates should target p95 fixed-update and presentation CPU below 8 ms for eight
  competitors at the agreed viewport configuration.
