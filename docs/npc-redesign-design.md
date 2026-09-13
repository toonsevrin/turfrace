# Encounter-first NPC redesign

## Orchestration decisions

The implementation brief supersedes illustrative APIs below in these respects:

- Keep all locally visible rivals up to `MAX_COMPETITORS`; segment budgets must preserve owner diversity and self-trail safety, not simply truncate the globally nearest list.
- Use a small independent competence model, not four replacement trait sliders. Seeded character policy must remain identical when only difficulty changes.
- Prediction may call `advance_motion` directly; extracting another movement API is unnecessary unless it improves the shared contract.
- Preserve existing outcome queue semantics unless additional event information is actually needed.
- Build the production-simulation laboratory alongside the tactical replacement. Versioned replays and bounded SVG trajectory artifacts provide an initial inspectable laboratory without coupling simulation to rendering.
- Acceptance requires actual trail cuts, owned-ground returns, pursuit abandonment, and territory theft; tactic labels alone are not evidence.

## Status and intent

This is a design proposal based on the current NPC and movement implementation. It
is deliberately an architecture and migration plan, not an implementation. The
authoritative movement, collision, capture, and replay rules remain shared with
humans.

The goal is for an NPC to appear to have a reason for what it is doing, notice and
respond to encounters, and sometimes make a consequential but understandable
mistake. It should not appear to re-run a numeric score calculation every few
frames.

## What exists today

The relevant current boundary is sound: `src/match_game/npc_systems.rs` owns ECS
queries and `src/npc/*` owns decision code. The main problems are inside that
boundary:

* `NpcTraits` is eight mostly independent floats. `skill` currently indirectly
  changes thinking rate, perception radius, commitment, and steering error, while
  the other floats are mixed into formulas in both the brain and planner.
* `NpcBrain` is a dynamic trait whose main implementation is
  `UtilityTactician`. `LegacyWanderer` is another complete brain selected by a
  random roster slot. This is a small strategy state machine disguised as a
  utility framework, with behavior difficult to author or inspect.
* `NpcController::decide` has a generic commitment timer, but the selected action
  and direction are effectively replaced on each thought. A capture is a fixed
  four-point rectangle and an interrupt is mostly a change to a direct steering
  vector.
* `NpcDecisionFrame` contains only three rivals and two owner-collapsed trails.
  `collect_trail_perceptions` scans nearby segment references, then searches the
  whole snapshot query for the owner for every reference. It also has a fixed
  28-unit collection radius unrelated to the frame's radius.
* The brain returns `home - position` for a return. There is no return route or
  validation that the selected route reaches a useful owned point, and finite
  turn rate means a bad return vector can spend time scraping the edge.
* `advance_motion` is the authoritative safe movement implementation, but there
  is no public prediction API using the same turn, boundary, and collision-margin
  behavior. NPC interception therefore cannot answer "where will this body be
  after a turn?" using the actual kinematics.
* `NpcMatchMemory` stores confidence, frustration, and a single estimate per
  opponent, but not a bounded sequence of events, locations, failed tactics, or
  the reason an NPC changed plan. The existing periodic steering error is
  intentionally colored but is not a meaningful mistake.
* Simulation outcome events already reach NPCs through `NpcEventQueue`, after
  capture/death/kill resolution and ranking update. That delivery point should be
  retained; it is a useful deterministic event boundary.

## Target model

### Profile: authored policy plus competence

Replace `NpcBrainKind` and `NpcTraits` with a profile containing two deliberately
different things:

```rust
pub struct NpcProfile {
    pub policy: NpcPolicy,
    pub competence: NpcCompetence,
}

pub enum NpcPolicy {
    Builder(BuilderPolicy),
    Hunter(HunterPolicy),
    Raider(RaiderPolicy),
}

pub struct NpcCompetence {
    pub control: f32,       // turn/aim quality and route following
    pub awareness: f32,     // sensor horizon and segment sampling quality
    pub judgment: f32,      // threat estimates and abort timing
    pub composure: f32,     // tolerance for a nearby threat before returning
}
```

The exact policy payloads should be small, named choices rather than more generic
floats. For example:

```rust
pub struct BuilderPolicy {
    pub shape: BuilderShape,       // Fill, Seal, BroadSweep
    pub side: TurnSide,
}
pub struct HunterPolicy {
    pub target: HunterTarget,      // Trail, ExposedRival, Opportunistic
}
pub struct RaiderPolicy {
    pub objective: RaidObjective,  // Leader, WeakestBorder, TrailCut
    pub shape: RaidShape,           // Hook, Wedge
}
```

A policy is the authored reason for acting. Competence is the imperfect execution
of that policy. Do not derive policy from competence, and do not pass every field
to every behavior. A builder's `shape` is consulted by the capture planner; it is
not silently converted into aggression, greed, exploration, and commitment.

`NpcCompetence` should have four documented consumers, as shown above. If a new
behavior needs a value, first decide whether it is policy (a reason/choice), a
world observation, or one of these execution limitations. Do not add another
cross-cutting scalar by default.

`generate_npc_roster` should choose a policy from authored weights and generate
competence within the difficulty bounds. Difficulty changes the bounds of
competence, not role priorities. A seeded roster should contain a visible mix of
roles at every difficulty. The old three-percent `LegacyWanderer` slot should be
removed; if its presentation is still wanted, map it to an authored
`BuilderShape::Roamer` during migration rather than keeping a second brain.

### Explicit tactic state

Use a concrete enum and a small state record, not `NpcBrain` and not a general
utility/consideration framework:

```rust
pub enum NpcTacticKind {
    Return(ReturnReason),
    Capture(CapturePurpose),
    Hunt(HuntTarget),
    Raid(RaidTarget),
    Roam,
}

pub enum TacticPhase { Acquiring, Travelling, Committing, Aborting }

pub struct NpcTactic {
    pub kind: NpcTacticKind,
    pub phase: TacticPhase,
    pub route: NpcRoute,
    pub route_index: u8,
    pub started_tick: u64,
    pub next_interrupt_tick: u64,
    pub max_duration_ticks: u32,
    pub mistake: Option<NpcMistake>,
}
```

`NpcRoute` is a bounded array of points (capacity eight is sufficient for the
first set of shapes), a count, and a final `RouteTarget`. It owns route progress;
it replaces `waypoints`, `waypoint_count`, and `waypoint_index`. The tactic's
minimum commitment is part of that tactic, rather than a controller-wide timer.

The controller becomes an ordinary concrete component:

```rust
pub struct NpcController {
    pub profile: NpcProfile,
    pub tactic: Option<NpcTactic>,
    pub memory: NpcEventMemory,
    pub think_remaining: f32,
    pub steering_error: f32,
    pub steering_error_target: f32,
    pub error_epoch: u32,
    pub safety_override: bool,
}
```

The public control operation should be one explicit transition function:

```rust
impl NpcController {
    pub fn tick(
        &mut self,
        observation: &NpcObservation,
        motion: CompetitorMotion,
        context: &NpcTickContext,
    ) -> NpcSteering;
}
```

`NpcTickContext` contains board/territory access, rank, fixed tick, and the
competitor's actual speed. `NpcSteering` contains a normalized desired direction,
source, and optional debug tactic label. It is not a scored list of alternatives.

The implementation of `tick` is intentionally readable and ordered:

1. Apply queued outcome memory and advance route progress.
2. Evaluate hard safety (`protected`, arena margin, dead/respawning, invalid
   route) and force a return/continue result where necessary.
3. Extract an `Encounter` from the bounded observation, including predicted
   rival/trail interception and the confidence of that estimate.
4. Interrupt the current tactic if the safety rule or the active policy's named
   interrupt rule says so. An encounter can interrupt a capture or roam; a
   low-confidence distant sighting cannot.
5. If there is no tactic, call exactly one role function:
   `choose_builder_tactic`, `choose_hunter_tactic`, or `choose_raider_tactic`.
6. Follow the current route using shared motion prediction and apply any selected
   bounded mistake.

The role functions should be ordinary `match`/`if` code with role-specific
predicates. They may use a few ordered rejection checks, but must not call a
common `score_actions` or introduce generic `Utility<T>`, `Consideration`,
`BehaviorNode`, or strategy traits. That keeps authored behavior debuggable and
prevents another trait soup.

An interrupt is allowed to replace a tactic immediately. Normal target changes,
wandering noise, and non-urgent policy changes wait for `next_interrupt_tick` or
route completion. This produces persistence without making an NPC blindly
committed.

## Perception and encounters

### Bounded segment perception

Replace owner-collapsed `NpcVisibleTrail` with a segment-level observation:

```rust
pub const NPC_VISIBLE_SEGMENT_CAP: usize = 24;

pub struct NpcVisibleSegment {
    pub owner: CompetitorId,
    pub segment: usize,
    pub nearest_point: Vec2,
    pub relative: Vec2,
    pub distance: f32,
    pub tangent: Vec2,
    pub own: bool,
}

pub struct NpcObservation {
    pub position: Vec2,
    pub heading: Vec2,
    pub protected: bool,
    pub owns_current_cell: bool,
    pub trail_length: f32,
    pub edge_distance: f32,
    pub inward_direction: Vec2,
    pub home: Option<Vec2>,
    pub rivals: [Option<NpcVisibleRival>; 4],
    pub segments: [Option<NpcVisibleSegment>; NPC_VISIBLE_SEGMENT_CAP],
}
```

The cap is a safety and design boundary, not just an optimization. Per viewer,
collect references from `BoardGrid::collect_nearby_trail_segments` within a
bounded horizon, resolve them through an array indexed by `CompetitorId` (the
same `MAX_COMPETITORS` bound already used by collision code), calculate nearest
points/tangents, sort by `(distance, owner, segment)`, and retain the first 24.
Do not search the ECS query once per reference. Include at most four rivals with a
stable `(distance, id)` order. A sensor horizon should be a bounded value derived
from awareness and clamped, for example `[16, 30]`; the board collection radius
and the final observation radius must be the same value.

`NpcObservation` should be built once per thinking NPC from an immutable
`NpcWorldSnapshot` assembled by `npc_think`. The snapshot contains each living
body's position, heading, speed, ownership area, exposure, and an optional trail
segment slice. This keeps ECS borrowing and perception policy separate and makes
perception tests pure. A local scratch vector may be used during collection, but
never allow an unbounded list into the controller.

### Shared prediction

Add a pure API to `src/movement/mod.rs` and make the existing step delegate to it:

```rust
pub fn predict_motion(
    motion: CompetitorMotion,
    desired: Option<Vec2>,
    territory: &TerritoryMap,
    config: &GameConfig,
    speed: f32,
    dt: f32,
) -> CompetitorMotion;

pub fn predict_position(
    motion: CompetitorMotion,
    desired: Option<Vec2>,
    territory: &TerritoryMap,
    config: &GameConfig,
    speed: f32,
    horizon: f32,
) -> Vec2;
```

`advance_motion` becomes a thin assignment wrapper around `predict_motion`. The
prediction must use the exact existing turn-rate clamp, arena margin, boundary
slide, inward correction, and finite-input handling. A multi-step forecast can
call `predict_motion` a bounded number of times (for example four 0.25-second
samples); do not duplicate movement math in the NPC planner.

`npc_think` should put actual speed in the body snapshot. A hunter predicts a
rival using its current heading and speed, then tests an intercept against the
NPC's own forecast. A segment encounter predicts the closest approach to the
segment tangent, rather than steering at the stale nearest point. A forecast is
an estimate, not knowledge of another player's input, and its confidence drops
with horizon and missing observations.

Add movement tests asserting that `predict_motion(...).position` equals the
position produced by `advance_motion` for normal, turn-limited, and boundary
contact cases. This is the contract preventing NPC-only kinematics from drifting
from authoritative movement.

### Safe returns

A return is a tactic with an actual route, not `home - position`.

Add a planner API such as:

```rust
pub fn plan_safe_return(
    board: &BoardGrid,
    territory: &TerritoryMap,
    id: CompetitorId,
    motion: CompetitorMotion,
    last_owned: Cell,
    threat: Option<&Encounter>,
    competence: NpcCompetence,
) -> NpcRoute;
```

The route builder should:

1. Prefer the nearest reachable owned frontier on the side of the current trail,
   with a small inward staging point before the final owned anchor.
2. Validate every route segment with bounded ownership samples and arena-margin
   checks. A route that cannot reach an owned point is rejected, not handed to
   steering as a direct vector.
3. Prefer a return frontier with the best forecasted threat clearance and shortest
   time. This is a small ordered search over owner frontiers, not a global utility
   score.
4. Use `LastOwnedCell` as the final fallback anchor, but first choose the safest
   inward direction and clamp the route to the arena margin. If no valid route is
   found, use a short emergency return tactic that re-plans next thought; it must
   never point outward at the edge.

When a return tactic is active, it can be interrupted only by death/protection
state or a clearly winning immediate encounter (for a hunter/raider). A builder
never abandons a safe return to chase. Re-plan after territory changes, an
encounter crossing its threshold, or a route point becoming invalid.

## Purpose-driven capture geometry

Replace the fixed `desired_depth`/`desired_width` rectangle in `planner.rs` with
an explicit request and shape generator:

```rust
pub enum CapturePurpose {
    FillFrontier,
    SealGap,
    CutTrail,
    RaidBorder,
}

pub struct CaptureRequest {
    pub purpose: CapturePurpose,
    pub target: Option<Vec2>,
    pub preferred_side: TurnSide,
    pub max_risk: f32,
}

pub struct CapturePlan {
    pub route: NpcRoute,
    pub purpose: CapturePurpose,
    pub estimated_area: f32,
    pub exit_distance: f32,
    pub return_clearance: f32,
}

pub fn plan_capture(
    context: &CapturePlanContext,
    request: CaptureRequest,
    scratch: &mut CaptureScratch,
) -> Option<CapturePlan>;
```

The first generators should be deterministic and role-authored:

* `FillFrontier` is a short adjacent strip or wedge selected from a local owner
  frontier. It is the builder's normal expansion and stays narrow when the return
  forecast is poor.
* `SealGap` joins separated nearby owner-frontier points with a compact hook.
  It is useful after a stolen area or for a builder responding to a local hole.
* `CutTrail` is a narrow intercept-shaped loop ending at the safest nearby owner
  frontier. It is the hunter's capture only when the target is about to leave a
  predictable exposed segment.
* `RaidBorder` is an asymmetric wedge or hook aimed toward the selected enemy
  border/leader. It is the raider's larger risk; its return leg is planned first,
  not added after the outbound geometry is chosen.

Generate candidates from frontier points and explicit shape parameters, reject
candidates that fail ownership, arena margin, route clearance, return-time, or
predicted encounter checks, then choose by a short lexicographic order: valid
return first, purpose fit second, area/length target third, stable id tie-breaker.
This is not a reusable utility evaluator. Geometry should vary with purpose,
frontier topology, target position, and current threat; it should not be a
single rectangle with `greed` and `risk` substituted into width and depth.

Remove `capture_risk_budget`. Risk belongs to a policy's named limit and to the
current `Encounter`/forecast. The plan should carry an estimated exit distance
and return clearance so interruption code can make a concrete decision.

## Event memory and meaningful mistakes

Keep the existing sparse outcome sources, but make the message and memory
explicit. Replace the tuple queue with:

```rust
pub struct NpcEventMessage {
    pub recipient: CompetitorId,
    pub event: NpcEvent,
    pub tick: u64,
}
pub struct NpcEventQueue(pub Vec<NpcEventMessage>);
```

`NpcEvent` should retain `Spawned`, `Died`, `OwnCapture`, `CreditedKill`, and
`TerritoryStolen`, and add a compact encounter result where the NPC itself has
completed or abandoned a hunt/raid. The message is queued only at tactic
boundaries and simulation outcomes, never once per visible segment.

Replace scalar-only memory with bounded, inspectable memory:

```rust
pub struct NpcEventMemory {
    pub recent: [NpcMemoryEvent; 8],
    pub recent_len: u8,
    pub opponents: [OpponentMemory; MAX_COMPETITORS],
    pub last_failure: Option<FailureMemory>,
}

pub struct OpponentMemory {
    pub observations: u16,
    pub threat: f32,
    pub last_seen_tick: u64,
    pub last_outcome: Option<EncounterOutcome>,
}
```

Record event kind, opponent, tick, and relevant area/position when available.
`on_event` receives the event tick; `record_encounter` receives the observed
encounter outcome and forecast confidence. Keep the ring fixed at eight and
keep opponent state bounded by `MAX_COMPETITORS`. Confidence/frustration can
remain as derived short-lived fields only if they are clearly tied to recent
records; do not add more emotional meters.

Mistakes should be rare, seeded, and tied to a tactic and situation. Add a small
controller-local deterministic stream (seeded from match seed, competitor id,
and tactic sequence) and an explicit enum:

```rust
pub enum NpcMistake {
    LateAbort,
    MisreadIntercept,
    OvercommitReturn,
    PoorSideChoice,
}
```

On tactic acquisition, roll only if the policy situation makes the mistake
plausible and the cooldown (for example 8--15 seconds) has elapsed. Competence
controls probability and magnitude; it must not change authoritative speed or
collision rules. Examples:

* `LateAbort` allows one extra think interval before a threatened capture aborts.
* `MisreadIntercept` uses a stale rival forecast for one route segment.
* `OvercommitReturn` delays choosing a closer return frontier, but still obeys
  arena safety.
* `PoorSideChoice` selects the less favorable frontier side, often reducing area
  or increasing exposure.

Record the mistake and its consequence in `NpcEventMemory`. Remove or repurpose
`update_colored_error`: visual steering drift may remain as a presentation/debug
field, but it must not be the gameplay mistake mechanism. Never inject random
invalid directions, disable boundary guards, or cause arbitrary self-kills.

## Exact file/API changes

### `src/npc/model.rs`

* Remove `NpcBrainKind`, `NpcTraits`, `NpcAction`, `NpcDecisionFrame`, and the
  generic waypoint/commitment fields from `NpcMatchMemory`.
* Add `NpcProfile`, `NpcPolicy`, the three role policy payloads, and
  `NpcCompetence`.
* Add `NpcObservation`, `NpcVisibleSegment`, `NpcEncounter`, `NpcTactic`,
  `NpcTacticKind`, `NpcRoute`, `CapturePurpose`, `NpcMistake`, and bounded
  event-memory types.
* Keep `NpcEvent` as the simulation-to-NPC vocabulary, changing queue entries to
  `NpcEventMessage` with a tick.
* Replace `CapturePlan`'s four fixed semantic waypoints with a bounded route and
  purpose/return metadata.

### `src/npc/brain.rs` (rename to `policy.rs`)

* Delete `NpcBrain`, `UtilityTactician`, and `LegacyWanderer`.
* Implement `choose_builder_tactic`, `choose_hunter_tactic`, and
  `choose_raider_tactic`, plus named interrupt functions. These functions return
  `NpcTactic`/transition values and do not score a list of utilities.
* Keep route following and emergency safety in the controller/planner, not in
  each policy function.

### `src/npc/planner.rs`

* Replace `propose_capture_plan` and `capture_risk_budget` with
  `plan_capture`, purpose-specific shape functions, and `plan_safe_return`.
* Reuse `segment_has_ownership`, but make route validation cover all route legs,
  arena margin, and a bounded forecast.
* Introduce `CaptureScratch` with explicit caps for frontier candidates and route
  candidates.

### `src/npc/sensing.rs`

* Replace `build_decision_frame` with `build_observation`.
* Build the fixed-size segment observations and `NpcEncounter` facts. Sensing may
  report facts and confidence; it must not choose a tactic.
* Add tests for cap enforcement, deterministic ties, same-radius filtering, and
  retaining multiple segments from one owner when they are the relevant nearby
  segments.

### `src/npc/roster.rs`

* Generate authored policy values independently from competence. Keep all random
  choices derived from the existing deterministic roster RNG.
* Replace tests for legacy probability and eight trait distributions with tests
  for role distribution, deterministic profiles, competence bounds, and stable
  policy selection.

### `src/npc/mod.rs`

* Replace the boxed brain in `NpcController` with `NpcProfile` and `NpcTactic`.
* Replace `prepare_thought`/`decide`/`write_steering` with `tick` and a small
  `write_steering` adapter (or have `tick` return `SteeringIntent`).
* Keep invalid-direction and arena-edge safety as a final controller guard.
* Keep `mix64` or move it into a small deterministic RNG helper used for mistakes;
  do not use a non-deterministic RNG in simulation.

### `src/match_game/npc_systems.rs`

* Build `NpcWorldSnapshot` once per invocation, including a direct
  `[Option<TrailSnapshot>; MAX_COMPETITORS]` lookup and bounded segment data.
* Replace `NpcPersonSnapshot`, `trail_perceptions`, `build_decision_frame`, and
  `propose_capture_plan` calls with `build_observation` and `controller.tick`.
* Pass `SimulationClock`, actual kill-adjusted speed, `LastOwnedCell`, and the
  shared movement context. Keep stable id sorting and staggered think cadence.
* Retain `NpcThink` ordering before movement. A controller may only write
  `SteeringIntent`; it must not mutate movement or territory directly.

### `src/movement/mod.rs`

* Extract current body of `advance_motion` into public pure `predict_motion` and
  add `predict_position`/bounded forecast.
* Have `advance_motion` assign the predicted result. Preserve all existing
  boundary constants and tests; add equivalence tests for prediction.
* Keep safe arena correction authoritative here. NPC-specific route safety stays
  in `npc/planner.rs`, so movement does not depend on NPC modules.

### `src/match_game/systems.rs`, `capture_systems.rs`, `outcomes.rs`, and `lifecycle.rs`

* Update every queue push to construct `NpcEventMessage` with the current tick.
* Keep `deliver_npc_events` after ranking/outcome systems, and pass message ticks
  into `controller.on_event`/memory. Clear the queue in lifecycle reset as today.
* Add sparse `EncounterCompleted`/`EncounterAbandoned` messages only where a
  tactic transitions; do not make perception a global event stream.
* Outcome semantics and equal-time capture ordering do not change.

### Call sites and diagnostics

* Update `lobby/mod.rs` and `match_game/lifecycle.rs` through
  `NpcController::from_roster`; no shell/device API should know about policy
  internals.
* Update `match_game/headless.rs` fingerprinting to include profile policy,
  competence, tactic kind/route index, memory records, and mistake state in a
  stable order. This is important for deterministic replay tests.
* Update `ui/gameplay_hud.rs` debug overlay: show `policy`, `tactic`, phase, route
  progress, current encounter, recent event/failure, and mistake cooldown rather
  than the old `traits` and `risk` dump. This is an essential tuning tool.
* Update exports in `src/npc/mod.rs`; avoid exposing implementation scratch types
  outside the NPC module.

## System ordering and integration risks

The intended fixed-update flow remains:

```text
PollInput -> NpcThink -> BuildSteeringIntent -> MoveCompetitors
 -> ExtendTrails -> collisions/deaths -> captures/territory -> rankings
 -> deliver NPC outcome events
```

`npc_think` can consume events only on the following tick, as it does today.
That one-tick latency is preferable to borrowing outcome state mid-resolution;
urgent arena safety remains local and immediate. If encounter transitions need
same-tick feedback, keep it as a local observation, not a queue round trip.

Main risks and mitigations:

* **Behavior regression from role migration.** Ship one role at a time and keep a
  temporary debug policy selector. Compare headless fingerprints and controlled
  captures, not only win rate.
* **Prediction disagreement.** Make `advance_motion` a wrapper around
  `predict_motion`; never copy its math into NPC code. Test edge contact and
  finite-turn cases first.
* **Route allocation or perception blow-up.** Use fixed caps, owner-indexed trail
  snapshots, stable sorting, and existing board buckets. Add a test with maximum
  trail references and assert no controller receives more than the cap.
* **Unsafe returns.** Validate ownership/arena legs and retain the existing final
  movement guard. Treat failure to find a route as an emergency return, never as
  permission to continue an invalid capture.
* **Event ordering/double delivery.** Keep one queue drain point, include the
  originating tick, and test death plus credited kill plus territory stolen in
  the same tick. Event memory writes must be idempotent only if an event id is
  added; otherwise preserve the current exactly-once queue contract.
* **Determinism.** Use `DeterministicRng`/`mix64` seeded by match seed and stable
  competitor/tactic sequence. Sort every candidate tie by IDs and segment index.
* **Overpowered encounter behavior.** Limit sensor horizon, forecast horizon, and
  visible segments. A target is actionable only when its predicted intercept is
  inside the horizon and has sufficient confidence; no NPC gets global state.
* **HUD/replay assumptions.** Update debug and fingerprint consumers in the same
  migration commit. Keep `SteeringIntent` and `CompetitorMotion` unchanged at
  the ECS boundary.

## Ordered implementation plan

1. **Freeze contracts and add characterization tests.** Add tests around current
   `advance_motion`, capture closure, queue ordering, deterministic headless
   runs, and NPC edge returns. Capture a few deterministic playtest scenarios for
   builder-like, hunter-like, and raider-like behavior.
2. **Extract shared prediction.** Implement `predict_motion` and its equivalence
   tests with no NPC behavior change. This is the lowest-risk prerequisite.
3. **Introduce profile and route data behind the existing controller.** Add
   `NpcProfile`, bounded `NpcRoute`, purpose enums, and roster generation while
   temporarily mapping old brain selection to a builder policy. Update HUD and
   fingerprints immediately.
4. **Replace sensing.** Build the immutable world snapshot, owner-indexed trails,
   fixed segment cap, and `NpcObservation`. Keep the old decision path consuming
   an adapter if needed; test cap, ordering, and performance bounds.
5. **Implement safe return as a standalone planner.** Make emergency return use it
   before changing capture or encounter priorities. Test no outward edge steering,
   reachable owned endpoint, and invalid-route fallback.
6. **Implement persistent tactics and the builder.** Migrate normal expansion to
   `FillFrontier`/`SealGap`, then remove generic commitment and fixed four-point
   capture behavior. Validate route progress and interrupt timing.
7. **Implement hunter and raider encounter policies.** Add forecast-based trail
   hunts, exposed-rival intercepts, and raider border/trail-cut shapes. Add explicit
   policy tests for when each role chooses, abandons, or returns.
8. **Add event memory.** Change queue messages and delivery, then record tactic
   outcomes and opponent-specific history. Test same-tick outcome delivery,
   bounded ring behavior, and non-adaptive profiles if that option is retained.
9. **Add meaningful mistakes and tune.** Add deterministic cooldown/probability,
   record mistakes, and verify they alter a plausible decision without violating
   movement/return safety. Tune policy payloads and competence bounds using the
   debug overlay and deterministic playtests.
10. **Delete compatibility code.** Remove the brain trait, legacy brain, old frame,
    risk utility, old waypoint fields, and adapter APIs only after headless,
    focused, and full feedback tests pass.

The result is a small explicit state machine: authored roles decide what matters,
competence limits how well it is executed, perception is bounded, movement is
shared, returns are routes, and memory explains the occasional change in
behavior. None of those responsibilities requires a utility framework or a web
of behavior traits.
