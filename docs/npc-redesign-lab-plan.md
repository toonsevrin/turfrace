# NPC redesign behavior laboratory plan

Status: design only. This document records the research baseline and the executable lab to build next; it does not change NPC behavior or add a second simulation.

## Research baseline

The authoritative path is already suitable for a behavior lab:

```text
MatchSpec -> start_simulation -> SimulationPlugin/FixedUpdate
  NpcThink -> SteeringIntent -> move_competitors -> trails
  -> swept combat -> deaths -> closures -> vector captures
  -> territory consequences -> victory -> respawns -> rankings
```

`HeadlessMatch` in `src/match_game/headless.rs` creates a device- and render-free Bevy `App`, acknowledges the current `MatchGeneration`, advances exactly one `FixedUpdate` per `step`, and returns a `ComparableSnapshot` plus drained `SimulationEvent`s. Snapshots already include position, heading, territory, trail length/fingerprint, life, score, and an NPC fingerprint. `HeadlessMatch::recording()` is a `MatchReplay`, but that replay contains only consumed human commands and the serializable `MatchSpec`; it does not describe post-start fixture mutations. The lab must therefore wrap a replay with its fixture setup, rather than pretending an empty NPC command stream is a complete replay.

NPC decisions are local and bounded. `npc_think` builds `NpcVisibleRival`/`NpcVisibleTrail` views, uses the owner-frontier cache, calls `propose_capture_plan`, then calls `NpcController::decide` and writes the common `SteeringIntent`. `NpcDecision` currently records action, direction, risk, commitment, and an optional plan. `NpcMatchMemory` retains four waypoints, confidence, frustration, commitment, and opponent estimates. There is currently no explicit `Bait` action or decision-reason field.

The production movement path in `src/movement/mod.rs` is forward movement with a capped turn rate, swept position (`previous_position` to `position`), and non-lethal arena-margin sliding. Trail collision is swept and resolved before closure. Capture is exact `MultiPolygon` geometry in `TerritoryMap`; `used_loop_fill`, `claimed_area`, and `stolen_by_owner` are authoritative results. The sample grid is only a cache for frontiers, rasterized trails, and rendering.

Existing coverage is useful but not sufficient for encounter diagnosis:

- `src/npc/model.rs`, `brain.rs`, `planner.rs`, `sensing.rs`, and `roster.rs` test bounded sensing, plan sizing, action commitment, deterministic roster generation, adaptation, and difficulty distributions.
- `src/match_game/tests.rs` has a 60 Hz multi-NPC capture-quality run and a separate accelerated one-hour invariant soak. The soak intentionally uses 10 Hz for collision stress and must not become the behavior score.
- `examples/visual_playtest.rs` runs the real shell and fixed-timestep game. Its `npc-match` scenario is a useful presentation starting point.
- The current debug-only F8 overlay in `src/ui/gameplay_hud.rs` shows action, risk, planned area, perception radius, waypoint progress, learned opponents, traits, and safety override. It has no reasons, plans drawn in world space, or trajectories.
- `docs/performance-benchmark.md` explicitly distinguishes native wall-clock diagnostics from browser FPS, memory, GPU, and fixed-system CPU measurements. The lab must preserve that distinction.

## Goals and non-goals

The lab must answer whether a candidate NPC redesign makes the same production racer behave legibly and robustly in repeatable encounters. It must expose *why* a decision was made, what path was actually driven after finite-turn movement and error, and what authoritative outcome followed.

It must not:

- call a brain in isolation as the primary acceptance test;
- replace movement, trails, combat, territory, respawn, or ranking with a miniature lab ruleset;
- judge personality by random win rate alone;
- use the ownership sample cache as a substitute for vector capture geometry;
- claim that a native Xvfb run is a browser or physical-GPU result;
- enable overlays or unbounded telemetry in normal release play.

Pure brain/planner tests remain appropriate for local invariants. Every encounter acceptance test must additionally run the real `SimulationPlugin` fixed schedule with real `NpcController`, `CompetitorMotion`, `SteeringIntent`, `ActiveTrail`, `TerritoryMap`, collision, closure, and respawn components.

## Proposed concrete surface

Add a small diagnostic module, preferably `src/npc/lab.rs`, and export only the data needed by the executable lab. It should provide:

```rust
pub enum EncounterFixture {
    ReturnRace,
    Interception,
    BaitDisengagement,
    DistractedTerritory,
    LoopCapture,
    BridgeCapture,
}

pub struct LabVariant {
    pub difficulty: NpcDifficulty,
    pub personality: PersonalityVariant,
    pub trace_capacity: usize,
}

pub struct LabReplay {
    pub format_version: u32,
    pub fixture: EncounterFixture,
    pub field_seed: u64,
    pub npc_roster_seed: u64,
    pub variant: LabVariant,
    pub spec: MatchSpec,
    pub human_commands: Vec<SteeringCommand>,
    pub ticks: u64,
}
```

`LabFixture::install(&mut App)` should create a normal `MatchSpec` with the fixture seeds, call `start_simulation` (or construct through `HeadlessMatch` and convert it with the existing `From<HeadlessMatch> for App`), acknowledge the generation, set the session to `Running`, and install only deterministic fixture state. Fixture state may use existing public APIs such as `TerritoryMap::apply_claim`, `BoardGrid::world_to_cell`, `CompetitorMotion::new`, `ActiveTrail::new`/`append_exact`, `update_trail_raster`, and component mutation. It must fail loudly if a requested point is outside the generated arena; it must not silently relocate a fixture.

The production setup should remain visible in the API: fixture installation is a *seeded initial condition*, not an alternate tick loop. A future `HeadlessMatch::into_app(self) -> App` convenience method is optional; the existing conversion already makes this possible.

## Seeded executable fixtures

Use the same 60 Hz `GameConfig` and disable victory for encounter windows so a fast capture cannot end the observation early:

```rust
spec.countdown_ticks = 0;
spec.rules = MatchRules { victory_enabled: false, ..MatchRules::from(&config) };
```

Set relevant spawn protection to zero only after installation. Keep normal player speed, turn rate, collision radius, trail width, self-exclusion distance, and respawn rules. Controlled human actors are legitimate puppets: their paths are supplied through the same `SteeringCommand` accepted by `HeadlessMatch`; the NPC under test is never moved by a fixture callback.

The following seeds are stable defaults, not claims that the fixtures have already passed. The first implementation must record the exact generated board fingerprint and reject any seed/position mismatch.

| Fixture | Field seed | NPC roster seed | Roster and controlled actor | Encounter setup and authoritative acceptance |
| --- | ---: | ---: | --- | --- |
| `return-race` | `0x4e50_4c41_4252_5201` | `0x4e50_4c41_4e52_5201` | `[Npc(0), Human(1)]`; human is the pressure actor | Place NPC 0 on its seeded turf, give it a known nearby frontier, and command Human 1 to enter the NPC perception radius while NPC 0 is outside. Assert a `ReturnHome` decision with `ThreatenedReturn` or `TrailBudgetReturn`, then either a surviving closure or a safe return with no `Death` for NPC 0. Measure time from threat observation to owned-territory re-entry. |
| `interception` | `0x4e50_4c41_4252_5202` | `0x4e50_4c41_4e52_5202` | `[Npc(0), Human(1)]`; human lays the exposed trail | Seed/position Human 1 so a commanded path creates an `ActiveTrail` across the NPC route. Hold NPC 0 near enough for `NpcVisibleTrail`, with aggression high enough to make the existing `HuntTrail` branch eligible. Assert `HuntVisibleTrail`, a swept `Death { cause: TrailCut }` for Human 1 and a credited NPC kill, or record a clearly bounded disengagement if the risk filter correctly refuses the cut. Never use direct `TrailCollisionIntent` as the encounter itself. |
| `bait-disengagement` | `0x4e50_4c41_4252_5203` | `0x4e50_4c41_4e52_5203` | `[Npc(0), Human(1), Human(2)]`; Human 1 is a hunter, Human 2 is a neutral witness | Command Human 1 to approach the NPC's exposed excursion without touching it. NPC 0 gets a safe outbound leg and then a rival close enough to threaten the return. The baseline fixture measures whether the existing planner holds the outbound commitment, then chooses `ReturnHome` rather than oscillating or blindly extending. A future explicit bait policy may label the same trace `BaitOpportunity`; no `Bait` action is assumed until added to the production model. Acceptance is a bounded disengagement, no self-cut, and a return/capture outcome, with the hunter's distance and NPC commitment recorded. |
| `distracted-territory` | `0x4e50_4c41_4252_5204` | `0x4e50_4c41_4e52_5204` | `[Npc(0), Human(1), Human(2), Human(3), Human(4)]`; Human 1 is the exposed leader | Use `TerritoryMap::apply_claim` to give Human 1 a deterministic large but non-winning area, ensure rankings put NPC 0 below the leader, and command Human 1 out of its area so it has an active trail. Place NPC 0 within perception range. Assert `StealExposedLeader` when that branch is eligible, then require any realized capture event to report `stolen_area > 0` without requiring the victim to be eliminated. This tests attention to a distracted leader, not merely final ranking. |
| `loop-capture` | `0x4e50_4c41_4252_5205` | `0x4e50_4c41_4e52_5205` | `[Npc(0), Human(1)]`; human is an optional geometry witness | Give NPC 0 one connected rectangular-ish turf lobe using `apply_claim`, install a production-valid way out and return path, and let the NPC follow its real capture plan. Require `SimulationEvent::Capture { loop_fill: true, .. }`, positive claimed area, and a changed vector territory. Also inspect the capture trace's contour/area descriptor, not just sample owner counts. |
| `bridge-capture` | `0x4e50_4c41_4252_5206` | `0x4e50_4c41_4e52_5206` | `[Npc(0), Human(1)]`; human is a non-interfering witness | Create two separated NPC-owned islands or a narrow bridge condition through public polygon claims, then drive NPC 0 through a path that reaches owned ground without a valid enclosing lobe. Require `loop_fill: false`, positive corridor area, no fabricated interior fill, and preservation of the disconnected/island geometry. The closure still goes through `detect_closures` and `resolve_captures`; the lab must not call `apply_claim` as a replacement for closure resolution. |

The setup helper should use deterministic named points (for example `npc_home`, `outbound_exit`, `return_exit`, `intercept_crossing`) derived from the field center and checked against `TerritoryMap::arena_signed_distance`, rather than selecting a new random point on failure. Initial claims must update `TerritoryRecord`/peak statistics to match the authoritative map before ticking. Active trails must be rasterized with `update_trail_raster`; manually inserting an un-rasterized trail would bypass the real broadphase and invalidate the encounter.

For each fixture, run a short explicit window (default 1,800 ticks / 30 seconds), stop early only on a declared terminal condition, and emit the seed, board generation identity, fixture setup hash, variant, and final `ComparableSnapshot`. A fixture failure should include the last 20 decisions and the relevant positions, not just `assert!(false)`.

## Decision reasons and trace contract

Add a reason to the production decision result rather than inferring it later from action names. A bounded enum should cover the current branches and the controller filter:

```text
Protected
BoundarySafety
ThreatenedReturn
TrailBudgetReturn
NoOwnedHomeReturn
ContinueWaypoint
HuntVisibleTrail
StealExposedLeader
BeginCapture
ExploreQuiet
CommitmentHeld
SafetyOverride
```

If the redesign introduces baiting, add `BaitOpportunity` and `DisengageThreat` only when the corresponding production branch exists. `CommitmentHeld` is important: the brain may propose a new action while `NpcController::decide` deliberately retains `last_decision` during commitment. Record both `brain_action` and `applied_action` so this distinction is visible.

A lab-enabled `NpcDecisionTrace` resource should retain a fixed-size ring buffer per NPC, with records containing:

- authoritative tick and match elapsed time;
- NPC ID, brain kind, difficulty/variant, brain action, applied action, and reason;
- position, heading, desired direction, steering error, and safety override;
- risk budget, commitment remaining, confidence, frustration, waypoint index/count, and planned area;
- bounded rival/trail observations (IDs, distances, exposed/own flags);
- capture-plan waypoints and estimated area when a plan is proposed.

The trace must be diagnostic only: no random draw, trait mutation, or steering change may depend on it. It should be disabled by default, bounded (for example 512 decisions per NPC and 256 motion samples), and omitted from normal release snapshots. The write point belongs next to `npc_think`/`NpcController::decide`, before `deliver_npc_events` drains the event queue. Persistent trace records are necessary in the shell because `presentation::simulation_events` drains `SimulationEvents`; in headless runs, `TickOutput.events` remains useful for replay assertions.

## Trajectory overlay

Extend the current debug-only F8 surface rather than creating a lab renderer. Keep the existing text overlay in `src/ui/gameplay_hud.rs`, but add a world-space diagnostic system in a new `src/render/npc_debug.rs` (or an equivalent debug-only presentation module). It should consume `NpcDecisionTrace` and current `CompetitorMotion`, not mutate simulation state.

For a selected NPC, draw a bounded, color-coded polyline of actual `motion.position` samples, with action/reason changes marked. Also draw:

- current heading and applied desired-heading vectors;
- `perception_radius` and visible rival/trail rays;
- active capture waypoints, home/return vector, and the proposed plan's staging/outbound/return/re-entry segments;
- arena inward normal when near the boundary;
- a distinct marker for steering-error direction and safety override;
- the exact active trail from `ActiveTrail::render_points` only as a visual aid, clearly separate from the bounded actual-motion trace.

Use a bounded debug mesh or Bevy gizmo line list in the X/Z gameplay plane at a small height above the production territory/trail surfaces. Do not draw every ownership cell or complete polygon on every frame. F8 remains off in release; a fixture selector and NPC filter should avoid putting twelve text rows and twelve full traces on top of a screenshot. `examples/visual_playtest.rs` should gain an opt-in NPC-lab scenario/fixture argument that starts the normal shell plugins, uses the same seeds, enables the overlay, and captures early, mid-encounter, and post-outcome frames. This is visual evidence only; the headless report remains the behavior authority.

## Maneuver statistics

Add a persistent, bounded `NpcManeuverStats` lab resource populated from authoritative transitions and events. Prefer event/state facts over heuristics:

- **Planning:** decisions, action transitions, plan proposals, commitment holds, plan waypoint progress, completed captures, aborted plans (with the trace reason), and planned versus realized area.
- **Return race:** trail start tick, first threat tick, return decision tick, owned re-entry tick, return duration, maximum excursion length, minimum rival distance, and whether the NPC survived/closed.
- **Interception:** visible enemy-trail ticks, `HuntVisibleTrail` duration, closest trail distance, approach path length, swept impact time, `DeathCause::TrailCut`, credited kill, or deliberate risk rejection.
- **Bait/disengagement:** duration of the safe exposed excursion, hunter visibility/closest approach, outbound commitment, `DisengageThreat`/`ReturnHome` transition, re-entry, self-cut, and opponent kill/capture outcomes. Do not call an excursion “bait” unless the trace has the explicit bait reason; the current code has no such action.
- **Distraction/theft:** leader rank at observation, leader exposed duration, `StealExposedLeader` time, capture `stolen_area`, victim remaining area, displacement, and whether the NPC returned safely.
- **Geometry:** loop/bridge result, claimed area, stolen area per owner, corridor length, lobe area, contour count/area/perimeter descriptor, and whether all non-target islands remain. Preserve the full claim only in an opt-in fixture artifact; keep steady-state telemetry scalar and bounded.
- **Motion quality:** actual path length, mean/max heading turn per second, boundary contacts, edge-slide time, max steering error, self-cuts, trail length, and fixed-tick count. These are diagnostic, not new scoring rules.

All ratios need denominators and zero-count handling in the report. Compare identical fixture/field/puppet commands across variants. Use action/reason distributions and authoritative outcomes as primary evidence; use territory or win rate only as secondary context.

## Difficulty and personality invariants

Difficulty and personality must remain orthogonal and deterministic:

1. For every `NpcDifficulty`, generated skill stays within the existing `skill_bounds`; the existing deterministic roster and at-most-one legacy-brain guarantees remain unchanged.
2. Easy/Normal/Hard change reaction frequency, perception radius, and steering-error bounds through `NpcTraits`, but do not change `GameConfig` speed, maximum turn rate, collision radius, trail width, capture geometry, or visibility rules. With the same kill count, `GameConfig::player_speed_for_kills` must be identical across difficulty.
3. In identical `NpcDecisionFrame`s, higher greed/risk pressure may select a larger plan; higher aggression may select visible-trail/leader actions; higher composure may tolerate more risk; commitment may lengthen action holds; turning bias may change side preference. These are directional decision invariants, not promises of winning.
4. Adaptive memory changes only after delivered authoritative `NpcEvent`s. A non-adaptive NPC must retain default opponent estimates; an adaptive NPC must record observations for death, credited kill, and territory theft exactly as current model tests specify.
5. `LegacyWanderer` remains an explicitly identified baseline, not a hidden personality mutation.

Implement these as matrix tests in `src/npc/lab.rs`/`src/npc` for pure frame-level properties and as full fixture tests in `src/match_game/tests.rs` for realized path/outcome properties. Every full fixture should run at least once with Normal plus a compact Easy/Hard smoke matrix. Do not require every difficulty to win or to produce the same capture count; require stable reason/action bounds, no invalid positions, no unexplained self-cut explosion, and preserved authoritative invariants.

## Executable command and replay

Add `examples/npc_lab.rs` with no `shell` requirement so it can run on the same headless build as `HeadlessMatch`. Add a small `scripts/npc-lab` wrapper with bounded defaults:

```sh
CARGO_BUILD_JOBS=2 cargo run --no-default-features --example npc_lab -- \
  --fixture return-race --difficulty normal --personality baseline \
  --ticks 1800 --trace target/npc-lab/return-race.json

scripts/npc-lab replay target/npc-lab/return-race.json --verify
```

The CLI should support `--fixture`, `--field-seed`, `--npc-seed`, `--difficulty`, `--personality`, `--ticks`, `--trace`, `--overlay` (shell build only), `--record`, and `replay PATH --verify`. Validate all values and cap ticks (for example 7,200 / two minutes) to prevent accidental unbounded lab jobs.

A recorded `LabReplay` stores the exact `MatchSpec`, fixture ID/setup version, both seeds, variant, tick count, and human puppet commands. Replay reconstructs the fixture, applies the same setup, advances the real fixed schedule, and compares per-tick `ComparableSnapshot`, drained events, and trace checkpoints. The command must reject a mismatched format/setup version and report the first divergent tick, NPC ID, action/reason, and snapshot field. This is a developer diagnostic command, consistent with `SPEC.md` keeping player-facing replay out of scope.

## Visual playtest integration

Keep `examples/visual_playtest.rs` as the shell integration test. Add an opt-in `npc-lab` scenario that uses `MatchSetup`/`start_match` and the same production plugins, not a synthetic arena. Its arguments should select the fixture and fixed capture frame. Use the current F8 overlay path for text and the planned world overlay for trajectories. Review at 1280x720 and 960x600, with captures at approximately 1 second (initial plan), 4 seconds (maneuver), and 8–10 seconds (outcome) for a 60 Hz run. Record exact output PNG paths. Overlay-on captures must never be used as performance measurements.

## Bounded browser performance verification

The headless lab proves behavior, not browser performance. Keep performance verification separate and honest:

1. Run `./scripts/feedback --quick` and the existing native `scripts/performance-benchmark` first. Use the existing release `performance_benchmark` at 1/2/4/8 views and `idle`/`capture-heavy`; do not interpret its Xvfb/Mesa wall-clock p95 as browser FPS or fixed-update CPU.
2. Build the real release site with `./scripts/build-web` (or `trunk serve --release`), serve `dist/` from HTTPS, and record the browser version, WebGL2 renderer, OS, viewport, device-pixel ratio, quality setting, and power-saving state.
3. Perform three cold loads, then at most four foregrounded 30-second passes (1, 2, 4, and 8 local views where hardware/controllers permit) at 1920x1080 Medium. Use the normal NPC-filled match and hold the same post-countdown window. Do a second bounded set only for the constrained/integrated tier or when a regression is observed. Keep lab tracing and overlays off for this pass.
4. For each pass, capture a 10-second Performance-panel trace after 10 seconds of warm-up. Record frame interval p50/p95/max, long tasks over 50 ms, WebGL/context-loss or shader errors, fixed-update/capture/death counts if exposed, and screenshots. If no real capture occurs, mark capture-heavy coverage incomplete rather than fabricating activity.
5. Repeat the 8-view pass on representative desktop, integrated, and constrained tiers; use Low/reduced motion only as a documented constrained compromise. Browser task-manager memory is an approximation and must remain labelled as such. Leave fixed-update CPU and 256 MB memory gates unverified unless a separate profiler measures them with its method and overhead stated.

This gives a bounded browser check of the actual renderer and simulation while preserving the repository's existing claims: behavior is reproducible headlessly, native timing is diagnostic, and browser/physical-device results require an explicit measured run.

## Build order and handoff gates

1. Add the fixture/setup schema and deterministic setup hash; first make loop and bridge geometry fixtures pass through production closure systems.
2. Add the persistent decision reason and bounded trace without changing steering output; update focused controller tests.
3. Add the executable lab, `LabReplay`, report serialization, and full ECS encounter tests.
4. Add maneuver counters and reason/action assertions for return, interception, disengagement, theft, and geometry.
5. Add the debug world overlay and `visual_playtest` opt-in captures.
6. Run bounded native checks, then the browser verification checklist. Before handoff, run the repository-required `CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 ./scripts/feedback` once. No behavior redesign should be accepted solely from a screenshot or an aggregate win-rate change.
