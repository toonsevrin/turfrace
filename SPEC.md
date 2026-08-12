# Turfrace v0.1 — Game Design and Technical Specification

## 1. Product definition

**Turfrace** is a competitive local multiplayer territory-control game for 2–8 human players. Players continuously steer colored cubes around a dynamically generated white playing field. Leaving owned territory creates a vulnerable trail. Returning to owned territory converts the trail into permanent territory and claims the enclosed area. Opponents can kill a player by touching that player’s active trail.

The match ends immediately when one competitor controls at least 95% of the claimable playing field.

The initial release is a static browser application built with Rust, Bevy, WebAssembly, and browser-local persistence. It has no server dependency and no online multiplayer.

### Normative language

* **MUST**: required for the initial release.
* **SHOULD**: expected polish unless performance or schedule requires postponement.
* **MAY**: optional enhancement.
* Values marked **configurable** must live in a central configuration resource rather than being scattered through systems.

## 2. Technical baseline

Pin the initial implementation to **Bevy 0.19.x**; Bevy 0.19 was released on June 19, 2026. Bevy’s official examples already demonstrate four 3D cameras rendering into separate viewports in one browser window, using WebAssembly and WebGL2, with a WebGPU variant available. ([Bevy][1])

The browser input adapter must support connection/disconnection events and polling of current controller state. A focused page may not expose an already connected gamepad until the user presses a button or moves an axis, which fits the proposed “press any button to join” lobby. ([MDN Web Docs][2])

Production deployment SHOULD use HTTPS because parts of the Gamepad API may be restricted to secure contexts. ([MDN Web Docs][3])

### Rendering targets

* **Baseline:** WebGL2.
* **Optional enhanced path:** WebGPU.
* Gameplay and shaders MUST work on the WebGL2 baseline.
* WebGPU-only effects MUST be cosmetic and automatically disabled when unavailable.
* The game canvas MUST resize to its parent and support fullscreen mode.

---

# 3. Initial release scope

## 3.1 Included

The initial version includes:

* 2–8 local human players.
* Mouse control for one player.
* Multiple simultaneous gamepads.
* NPC competitors explicitly added to the field.
* Configurable robot count.
* Dynamically generated field contours.
* Territory capture and stealing.
* Trail cutting and self-trail collisions.
* Escalating respawn times.
* Split-screen cameras.
* Home, lobby, leaderboard, settings, pause, and results screens.
* Browser-local profiles and statistics.
* Three modular NPC behavior styles.
* Basic sound, particles, camera effects, and lightweight shaders.
* Colorblind identification patterns.
* Static web deployment.

## 3.2 Explicitly out of scope

The following are not part of v0.1:

* Online multiplayer or networking.
* Cloud profiles or online leaderboards.
* User accounts.
* Matchmaking.
* Touchscreen controls.
* Mobile portrait layouts.
* Power-ups or character abilities.
* Obstacles inside the field.
* Multiple game modes.
* User-created maps.
* Replay viewing as a player-facing feature.
* Physics-engine-driven gameplay.

A deterministic developer replay log is recommended for debugging, but it does not require a user interface.

---

# 4. Default configuration

| Setting                             |                             Default |
| ----------------------------------- | ----------------------------------: |
| Human players                       |                                 2–8 |
| Robots                              |                                   0 |
| Total competitor range              |                                2–12 |
| Total competitors                   |             `human players + robots` |
| Simulation frequency                |             60 fixed updates/second |
| Player speed                        |              8.0 world units/second |
| Kill speed bonus per kill            |                                  3.5% |
| Maximum kill speed bonus             |                                   28% |
| Maximum turn rate                   |                         270°/second |
| Cube colored core size              |                          1.20 units |
| Cube black outline size             |                          1.36 units |
| Cube collision radius               |                          0.52 units |
| Trail width                         |                          0.65 units |
| Trail visual opacity                |                                 50% |
| Territory height                    |                          0.10 units |
| Ownership grid cell size            |                          0.50 units |
| Starting territory radius           |                          2.75 units |
| Spawn protection                    |                        1.25 seconds |
| First respawn delay                 |                           5 seconds |
| Respawn increase per death          |                           5 seconds |
| Victory territory threshold         |                                 95% |
| Leaderboard rows                    |                               Top 3 |
| Camera downward angle               |  Approximately 68° below horizontal |
| Camera vertical field of view       |                                 48° |
| Gamepad radial deadzone             |                                0.18 |
| Mouse deadzone                      |                    24 screen pixels |
| Standard NPC think frequency        |                                8 Hz |

The final respawn rule supersedes the earlier fixed ten-second proposal:

```text
respawn_delay_seconds = 5 × number_of_deaths_this_match
```

Therefore:

* First death: 5 seconds.
* Second death: 10 seconds.
* Third death: 15 seconds.
* Fourth death: 20 seconds.
* No cap in v0.1.
* Humans and NPCs use exactly the same formula.

The cap must remain configurable so playtesting can introduce one later without changing gameplay code.

---

# 5. Core match rules

## 5.1 Competitors

A competitor is either:

* A human-controlled player.
* An NPC-controlled player.

Every competitor has:

* A stable match ID.
* A display name.
* A primary color.
* A secondary identification pattern.
* A current position and heading.
* A territory cell count.
* An optional active trail.
* Kill and death statistics.
* A respawn death count.
* A life state.
* A ranking position.

Human players receive split-screen viewports. NPCs do not.

## 5.2 Match start

At match start:

1. Generate the field from the match seed.
2. Determine non-overlapping spawn locations.
3. Assign every competitor a circular starting territory.
4. Place each cube at the center of its territory.
5. Point initial headings approximately toward the center of the field.
6. Apply spawn protection.
7. Display a shared `3 – 2 – 1 – GO` countdown.
8. Begin simulation after `GO`.

All competitors spawn simultaneously.

## 5.3 Continuous movement

A living cube always moves forward.

Input does not directly set velocity. It sets a **desired heading**.

Each fixed update:

1. Read the desired direction.
2. Rotate the current heading toward it.
3. Limit rotation by the maximum turn rate.
4. Move forward at the configured constant speed.

When input is inside the deadzone, the cube retains its existing heading.

This creates predictable motion and prevents instantaneous 180-degree reversals.

## 5.4 Field boundary behavior

The field edge is a solid, non-lethal boundary.

A cube that attempts to leave the field:

1. Is clamped to the nearest valid interior position.
2. Has its outward velocity component removed.
3. Continues along the edge tangent.
4. Receives a small inward steering correction.

If the cube is aimed almost directly outward and no valid tangent remains, select the tangent closest to its previous heading.

Hitting the field edge does **not** kill a player.

A subtle edge-warning effect SHOULD appear when the player is less than 1.5 units from the boundary and is steering outward.

## 5.5 Cube-to-cube contact

Cubes are non-solid relative to one another.

Direct cube contact:

* Does not kill either player.
* Does not cause a bounce.
* Does not alter heading.
* Does not block movement.

All combat is based on active trails and loss of territory. This avoids ambiguous head-on outcomes and keeps the rule set readable.

---

# 6. Territory and trail rules

## 6.1 Safe movement

A player is considered safe while the center of their cube is inside that player’s exact vector
territory. The ownership grid mirrors this state for trail rasterization and broadphase only.

While safe:

* No active trail is drawn.
* The player may move freely through the single territory island connected to their spawn anchor.
* Opponents may still steal the cell from underneath them through a capture.

### 6.1.1 Single-island invariant

Each competitor stores the center of their current spawn seed as a persistent anchor. After every
committed capture, territory polygons that no longer contain that anchor are removed. A competitor
standing on removed geometry dies with cause `Displaced`; a competitor still on the anchor-connected
polygon is unaffected. This invariant makes it impossible for one competitor to own two islands.

## 6.2 Starting a trail

A trail starts when the player’s center crosses from exact owned territory into space that is:

* Unclaimed, or
* Owned by another competitor.

The system records:

* The exact interpolated field position at which the boundary was crossed.
* The last owned grid cell.
* The initial trail point.
* The current trail length.
* Trail cells rasterized into the collision grid.

The trail is immediately vulnerable.

## 6.3 Active trail behavior

An active trail:

* Is visually a flat ribbon.
* Has zero geometric thickness in the vertical dimension.
* Uses the player’s primary color at 50% opacity.
* Is collision-active over its configured width.
* May cross unclaimed or enemy-owned territory.
* Remains active until the player closes it or dies.
* Does not itself alter territory ownership before closure.

The player’s current head naturally overlaps the most recent portion of their own trail. Self-collision therefore ignores the most recent 1.5 world units of that trail.

## 6.4 Closing a trail

A trail closes when the player enters any currently owned cell.

There are two closure outcomes.

### Loop capture

If a path currently exists through the player’s territory between:

* The owned cell at which the trail started, and
* The owned cell at which it ended,

then the trail and the shortest path through the existing territory form a closed polygon.

The system:

1. Fills the polygon.
2. Claims every playable cell whose center lies inside it.
3. Claims the entire trail corridor.
4. Steals enclosed cells from opponents.
5. Removes the active trail.

### Bridge capture

If no owned path exists between the start and end cells, the trail did not form a valid enclosed region. This can occur when:

* The player reaches a narrow bridge or corridor without enclosing an interior.
* Opponents stole the trail’s original anchor.
* The original territory was split while the player was outside.

In this case:

* Only the trail corridor becomes permanent territory.
* No polygon interior is filled.
* The resulting strip may reconnect territory islands.
* The active trail is removed.

This rule prevents geometry failures while giving the player a small reward for successfully reaching owned land.

## 6.5 Self-trail collision

If a player intersects their own active trail outside the recent-head exclusion distance:

* The player dies.
* The trail is cancelled.
* No capture occurs.
* No opponent receives kill credit.

This prevents self-intersecting capture polygons and makes reckless loops dangerous.

## 6.6 Stealing territory

Capturing a cell replaces its previous owner.

There is no defensive strength or capture delay. Ownership is always one of:

* Unclaimed.
* Owned by exactly one competitor.

If a capture steals the cell currently occupied by another player who was not drawing a trail:

* That player immediately transitions to the drawing state.
* Their active trail begins at their current position.
* They are not killed solely because the ground changed owner.

If a player’s territory count reaches zero:

* The player dies with cause `Displaced`.
* Their active trail is removed.
* The competitor responsible for the final territory removal receives kill credit.
* Their normal escalating respawn begins.

Capturing territory around another player’s cube does not otherwise kill that player.

## 6.7 Active trails inside captured areas

Capturing territory containing another player’s active trail:

* Does not remove the trail.
* Does not automatically kill its owner.
* Does not make the trail safe.
* Does not transfer trail ownership.

Trail combat remains independent from underlying territory ownership.

---

# 7. Combat and death

## 7.1 Cutting a trail

When a cube’s swept collision shape intersects another player’s active trail:

* The owner of that trail dies.
* The intersecting player receives one kill.
* The killer survives unless separately killed in the same fixed update.

The game must use swept collision between the cube’s previous and new positions so high speed cannot skip over a trail.

## 7.2 Collision timing

Trail collisions are resolved **before** trail closures.

Therefore, if a player reaches their territory in the same update that an opponent cuts their trail:

* The player dies.
* The capture does not complete.

This favors successful attackers and avoids trails disappearing before a valid collision is processed.

## 7.3 Simultaneous kills

All collision intents are collected before any death is applied.

Consequences:

* If Player A cuts Player B and Player B cuts Player A in the same update, both die.
* A player killed during the update may still receive credit for a cut they made in that update.
* A trail owner is killed only once even if several opponents touch the trail simultaneously.

When multiple attackers touch the same trail in one update, kill credit goes to:

1. The attacker with the earliest swept impact time.
2. If tied, the attacker with the lowest stable competitor ID.

## 7.4 Death effects

On death, gameplay state changes immediately:

* The cube becomes inactive.
* The active trail is removed.
* Every territory cell belonging to that player becomes unclaimed.
* Territory count becomes zero.
* Death count increments.
* The respawn timer is set.
* Input no longer affects movement.

Visual disappearance MAY take 0.3–0.5 seconds using non-authoritative fade or dissolve effects. Other players must be able to claim the newly unclaimed cells immediately even while the visual fade is finishing.

Recommended death presentation:

* Cube breaks into small colored fragments.
* Trail rapidly fades toward its origin.
* Claimed territory dissolves to white.
* Victim viewport receives a short camera shake.
* Killer viewport receives a small confirmation flash.
* A compact kill-feed message appears.

## 7.5 Kill momentum and streaks

Each credited kill increments the killer's total kill count and current kill
streak. The current streak resets when that competitor dies; the best streak
remains match statistics. Total kills increase movement speed by 3.5% each,
up to a 28% cap, and the bonus persists through respawns.

The killer receives escalating non-authoritative feedback as total kills rise:

* A colored particle burst and ground ring appear at the killer's position.
* The killer's local camera briefly punches its FOV, while total kills and the
  current streak also widen the persistent gameplay FOV.
* The quiet HUD relies on cube lean, trail treatment, camera response, and audio
  rather than numeric speed, kill, or streak telemetry.

## 7.6 Spawn protection

For 1.25 seconds after spawning:

* The player cannot be killed.
* The player cannot kill another competitor by cutting a trail.
* The player does not create a trail.
* The cube visibly pulses or displays a shield ring.

Protection ends when either:

* The timer reaches zero, or
* The player leaves the starting territory after at least 0.5 seconds.

This prevents spawn camping without providing a long offensive advantage.

---

# 8. Respawning

## 8.1 Respawn sequence

While dead:

* The player’s viewport remains present.
* A centered countdown displays tenths of a second below five seconds.
* The camera initially remains near the death location.
* It gradually zooms out during longer respawns.
* Shortly before spawning, it transitions to the new spawn point.

NPCs use the same respawn system but have no viewport.

## 8.2 Respawn location selection

At the moment of respawn, generate candidate positions inside the field.

A valid preferred candidate must:

* Have enough boundary distance for the complete starting territory.
* Be at least 12 units from any living cube.
* Be at least 6 units from any active trail.
* Not overlap another respawning player’s seed area in the same update.
* Prefer unclaimed territory.
* Prefer locations far from the current leader.

Recommended algorithm:

1. Sample 256 valid field cells using the deterministic match RNG.
2. Reject cells that violate hard safety distances.
3. For each candidate, calculate:

   * Percentage of the proposed seed disk that is unclaimed.
   * Minimum distance to living cubes.
   * Minimum distance to active trails.
   * Distance from the current leader.
   * Distance from the field edge.
4. Choose the highest-scoring candidate.
5. If none exists, relax cube and trail distances in two stages.
6. As a final fallback, choose the safest valid interior cell.

The new seed territory may overwrite a small number of claimed cells if no unclaimed spawn region remains. This acts as a limited comeback mechanism.

## 8.3 Respawn ordering and victory

Victory is checked before expired respawn timers create new territory.

If a competitor reaches the victory threshold during the same update in which another player is due to respawn:

* The match ends.
* The respawn does not occur.

---

# 9. Victory and ranking

## 9.1 Territory percentage

A competitor’s territory percentage is:

```text
owned_vector_area / total_arena_vector_area × 100
```

Area outside the generated contour is never counted.

Display percentages to one decimal place, but use exact fixed-point polygon areas for all comparisons and victory checks. The sample grid is not authoritative for victory.

Player-facing territory percentages normalize the victory threshold to 100%:
`display_percent = min(actual_percent / victory_threshold_percent × 100, 100)`.

## 9.2 Live ranking

All match participants are ranked by:

1. Territory vector area, descending.
2. Living competitors before respawning competitors.
3. Kill count, descending.
4. Stable competitor ID, ascending.

The leaderboard displays the top three overall competitors, not the top three humans.

Examples:

* Two humans and six NPCs: an NPC in the top three appears normally.
* Eight humans and no NPCs: the top three humans appear.
* Fewer than three total competitors: display all competitors.

Local viewports do not duplicate rank and percentage telemetry; players read their standing from
the shared leaderboard and their cube's color/name cues.

Respawning players remain in the full rankings at zero territory and are shown dimmed if they appear due to a very small competitor count.

## 9.3 Leader indicator

The living competitor ranked first SHOULD have a small floating crown, ring, or chevron above their cube.

The indicator must:

* Be visible from all cameras.
* Transfer when the ranking changes.
* Use no gameplay collision.
* Avoid obscuring the cube.

## 9.4 Victory condition

Victory occurs when:

```text
owned_vector_area[winner] × 100 >= total_arena_vector_area × victory_threshold_percent
```

The default threshold is configurable through `GameConfig` and is 95% in v0.1. There is no rounded-percentage shortcut, and opponents may still own the remaining area.

When victory occurs:

1. Freeze gameplay simulation.
2. Remove further input effects.
3. Display the winner announcement in all viewports.
4. Play the victory effect.
5. Hold the final field for approximately three seconds.
6. Open the results screen.

Results rank the remaining players using:

1. Highest territory percentage reached during the match.
2. Kill count.
3. Fewest deaths.
4. Stable competitor ID.

---

# 10. Dynamic field generation

## 10.1 Coordinate system

* Gameplay takes place on the XZ plane.
* Y is vertical.
* Logical gameplay positions use `Vec2(x, z)`.
* Rendering converts them to `Vec3(x, y, z)`.
* The field center is logical `(0, 0)`.

## 10.2 Field size

Field size scales with total competitor count.

Recommended target area:

```text
target_area = 4000 + 700 × total_competitors
equivalent_radius = sqrt(target_area / π)
```

Approximate examples:

* 2 competitors: radius about 41.5 units.
* 8 competitors: radius about 55.3 units.
* 12 competitors: radius about 62.8 units.

The generated contour is normalized after deformation so its final playable area remains close to the target.

## 10.3 Contour generation

The contour must be rounded, smooth, irregular, and deterministic from the match seed.

Generate 128 angular boundary samples.

Start with a superellipse:

```text
base_radius(θ) =
1 / (
  |cos(θ) / a|^p +
  |sin(θ) / b|^p
)^(1/p)
```

Where:

* `p` is randomly selected from 2.2 to 3.6.
* Aspect ratio is selected from 0.88 to 1.12.
* `a` and `b` are chosen around the equivalent radius.

Apply smooth low-frequency deformation:

```text
deformation(θ) =
1 + Σ amplitude[k] × cos(k × θ + phase[k])
```

For `k = 2..6`.

Constraints:

* Combined radial deformation must remain between 0.88 and 1.12.
* No sharp points.
* No holes.
* No disconnected field sections.
* No neck narrower than two starting-territory diameters.
* The contour remains star-shaped around the center.

Because the contour is star-shaped, the visible field mesh can be triangulated with a center fan.

## 10.4 Field mask

The contour is converted to a grid:

```rust
struct BoardGrid {
    width: u32,
    height: u32,
    cell_size: f32,
    world_origin: Vec2,
    field_mask: Vec<bool>,
    owner: Vec<OwnerId>,
    active_trail_bits: Vec<u16>,
    signed_distance: Vec<f32>,
    owner_counts: Vec<u32>,
    dirty_chunks: BitSet,
}
```

Recommended representation:

* `field_mask[index] == false`: outside field.
* `owner[index] == 0`: playable but unclaimed.
* `owner[index] == 1..=12`: competitor ownership.
* `active_trail_bits[index]`: one bit per competitor.
* Signed distance is positive inside the field and negative outside.

The signed-distance grid supports:

* Boundary sliding.
* Spawn margins.
* Edge warning effects.
* Fast inside/outside queries.

## 10.5 Initial spawn distribution

Initial spawn points use farthest-point sampling:

1. Build candidate cells with at least eight units of field-edge clearance.
2. Pick the first point from the seeded RNG.
3. Repeatedly choose the candidate with the greatest minimum distance to existing spawn points.
4. Stop when all competitors have a point.
5. If minimum spacing is insufficient, regenerate the contour or enlarge the field.

Starting seed disks must not overlap.

---

# 11. Authoritative capture algorithm

Gameplay is continuous visually, but exact fixed-point vector multipolygons are authoritative for
territory ownership. The low-resolution grid is a derived cache for rendering and broadphase work;
it never decides containment, capture, ranking, or elimination.

## 11.1 Trail sampling

Append a trail point when either condition is met:

* The cube has moved at least 0.20 units from the latest point.
* The heading changed by at least 6°.

Always append the exact current point before collision and closure resolution.

Simplify old trail points only when doing so cannot change collision coverage. A safe option is to retain all points for gameplay and maintain a separately simplified visual mesh.

## 11.2 Trail rasterization

Rasterize each line segment as a capsule with radius:

```text
trail_width / 2
```

The rasterized trail must be at least eight-connected so diagonal segments do not have gaps.

For each player, track:

* Unique trail cells.
* Segment sequence numbers.
* Recent trail cells excluded from self-collision.
* Total trail length.

## 11.3 Closing path

When the cube crosses back into the player’s exact vector territory, interpolate the boundary
entry time and build a stroked corridor from the active trail points. If the trail intersects a
current outer contour, choose the smaller valid lobe bounded by the trail and that contour. The
candidate is clipped to the arena and unioned with the corridor. If no valid lobe exists, the
corridor alone is committed as a bridge capture.

## 11.4 Vector capture

Capture geometry uses deterministic fixed-point `MultiPolygon` booleans:

1. Stroke the trail into a capsule corridor.
2. Union the corridor with the selected loop lobe, when present.
3. Intersect the result with the arena.
4. Difference the claim from every other owner.
5. Union the claim into the capturing owner.
6. Retain only each owner’s polygon containing its spawn anchor; report removed polygons.

The sample grid is refreshed only over changed geometry AABBs for rendering and broadphase. Exact
vector areas and containment remain authoritative for statistics, displacement, ranking, and the
95% victory threshold.

## 11.5 Applying ownership

After a committed vector mutation:

* Clear the capturing player’s active trail and its raster bits.
* Emit displacement credits for owners reduced to zero or cubes standing on severed polygons.
* Start trails for safe competitors whose current ground was stolen.
* Refresh changed board-cache AABBs and owner mesh revisions.
* Emit capture statistics and visual events.
* Recalculate rankings and check victory.

## 11.6 Closure pseudocode

```rust
fn resolve_closure(player: PlayerId, map: &mut TerritoryMap, trail: &ActiveTrail) {
    let result = map.calculate_capture(player, trail, TRAIL_WIDTH);
    let committed = map.apply_claim(player, result.claim);
    refresh_changed_cache_aabbs(&committed);
    clear_active_trail(player);
    kill_cubes_on(committed.disconnected_by_owner);
}
```

---

# 12. Fixed-update ordering

Gameplay runs in Bevy’s `FixedUpdate` schedule at 60 Hz.

Systems must be grouped into explicit ordered sets:

1. **PollInput**

   * Read gamepads, mouse, keyboard fallback, and UI actions.

2. **NpcThink**

   * Update NPC plans when their individual think timers expire.

3. **BuildSteeringIntent**

   * Convert human or NPC input into desired direction.

4. **MoveCompetitors**

   * Rotate headings.
   * Apply movement.
   * Resolve field boundary sliding.
   * Store previous and current positions.

5. **ExtendTrails**

   * Detect exits from owned territory.
   * Begin or append trails.
   * Update trail collision cells.

6. **DetectTrailCollisions**

   * Sweep cubes from previous to current positions.
   * Generate external-cut and self-cut intents.

7. **ResolveDeaths**

   * Apply all simultaneous deaths.
   * Clear territory and trails.
   * Award kills.

8. **DetectClosures**

   * For surviving players, detect entry into owned cells.
   * Calculate sub-frame closure time.

9. **ResolveCaptures**

   * Apply captures in chronological order.
   * Equal-time overlapping captures use deterministic conflict resolution.

10. **ResolveTerritoryConsequences**

    * Start newly exposed trails.
    * Kill players reduced to zero territory.

11. **CheckVictory**

    * Test exact ownership counts.

12. **AdvanceRespawns**

    * Tick timers.
    * Spawn eligible competitors only if the match has not ended.

13. **UpdateRankings**

    * Emit ranking-change events as required.

Camera movement, animations, UI interpolation, particles, and audio run in the ordinary `Update` schedule.

## 12.1 Simultaneous captures

Multiple players may close trails during one fixed update.

Each closure records an estimated entry time `t` from 0 to 1 along that update’s movement.

* Captures with different `t` values are applied in chronological order.
* A later capture may steal cells captured earlier in the update.
* Captures whose times are equal within a small epsilon are calculated from the same board snapshot.
* A cell claimed by multiple equal-time captures goes to the lowest competitor ID.

---

# 13. Input system

## 13.1 Input abstraction

Gameplay systems must not know whether steering came from a mouse, gamepad, keyboard, or NPC.

All controllers produce:

```rust
struct SteeringIntent {
    desired_direction: Vec2,
    magnitude: f32,
    source: ControlSource,
}
```

Movement consumes only `SteeringIntent`.

## 13.2 Supported device types

```rust
enum InputDeviceId {
    Gamepad(u32),
    Mouse,
    KeyboardPrimary, // Development and accessibility fallback
}
```

Only one player may own the mouse.

Every physical gamepad may be assigned to only one player.

## 13.3 Gamepad gameplay controls

Default standard layout:

* Left stick: steer.
* Right stick: optional alternate steering input.
* Start/Menu: pause.
* South/A/Cross: confirm in menus.
* East/B/Circle: back or cancel.
* D-pad and left stick: menu navigation.
* Left/right shoulder: quick color cycling in lobby.

Stick processing:

1. Read the two-dimensional axis.
2. Apply radial deadzone 0.18.
3. Rescale the remaining range to 0–1.
4. Convert screen-oriented direction to world direction.
5. Retain current heading when below the deadzone.

Because cameras are north-locked, screen-oriented and world-oriented steering remain consistent.

## 13.4 Mouse gameplay controls

The mouse player steers toward the point under the cursor.

Each frame:

1. Clamp the cursor to that player’s viewport.
2. Cast a ray from the player’s camera through the cursor.
3. Intersect the ray with the gameplay plane.
4. Calculate direction from the cube to that world point.
5. If the screen-space distance from cube to cursor is less than 24 pixels, retain the current heading.

The mouse cursor remains visible by default.

A relative pointer-lock aiming mode MAY be added as a setting but is not required.

## 13.5 Keyboard fallback

Keyboard steering is not advertised as a core multiplayer input, but SHOULD exist for development and accessibility:

* WASD: desired direction.
* Escape: pause/back.
* Enter/Space: confirm.

Keyboard steering produces an eight-direction desired heading and still obeys turn-rate limits.

## 13.6 Device disconnection

During the lobby:

* The slot is marked disconnected.
* The profile and color remain reserved.
* An unassigned controller may reclaim the slot.
* The player may be removed by another joined player.

During a match:

* Pause the entire match.
* Show which player lost connection.
* Allow any unassigned controller to take over that slot.
* Allow replacing the disconnected player with an NPC.
* Allow quitting to the lobby.

The game must never leave a disconnected human cube moving unattended.

---

# 14. Split-screen and cameras

## 14.1 Viewport layouts

Only human players receive viewports.

Use the following landscape layouts:

|              Humans |  Grid | Placement                     |
| ------------------: | ----: | ----------------------------- |
| 1, development only | 1 × 1 | Full screen                   |
|                   2 | 2 × 1 | Side by side                  |
|                   3 | 2 × 2 | Two top, one centered below   |
|                   4 | 2 × 2 | Full grid                     |
|                   5 | 3 × 2 | Three top, two centered below |
|                   6 | 3 × 2 | Full grid                     |
|                   7 | 3 × 3 | Three, three, one centered    |
|                   8 | 3 × 3 | Three, three, two centered    |

Each viewport has:

* A two-pixel neutral divider.
* A thin top accent using that player’s color.
* Its own 3D camera.
* Its own local HUD root targeted to that camera.

## 14.2 Camera orientation

The camera is almost top-down but retains visible perspective.

Default transform relative to the player:

```text
camera offset = (0, 27, 11)
look target   = player position + 2.5 units along heading
vertical FOV  = 48°
```

The camera:

* Keeps a fixed world yaw.
* Does not rotate with the player.
* Smoothly follows using an exponential half-life of approximately 0.12 seconds.
* Uses a small forward look-ahead.
* Never crosses the field plane.

## 14.3 Dynamic zoom

The camera may zoom out by up to 25% based on:

* Active trail length.
* Distance from owned territory.
* Proximity to the field edge.

Zoom changes must be slow and subtle.

It must not zoom in while the player is drawing a long trail, because the player needs awareness of the route home and approaching attackers.

All human players use equivalent world coverage after compensating for viewport aspect ratio.

## 14.4 Camera while dead

On death:

* Hold near the death location for the initial effect.
* Ease outward to show more of the field.
* Keep the respawn countdown centered.
* Transition rapidly but smoothly to the respawn location when spawning.

No automatic spectating of another player is required.

## 14.5 Screen-space indicators

Each player viewport SHOULD show:

* An arrow toward the nearest owned territory while drawing.
* A warning marker for an enemy cube close to the player’s trail.
* An inward edge arrow when dangerously close to the boundary.

Indicators must fade when their target is visible.

---

# 15. User interface

## 15.1 Global application states

```rust
enum AppState {
    Boot,
    Home,
    Lobby,
    MatchLoading,
    Countdown,
    Playing,
    Paused,
    GameOver,
    Results,
    LocalLeaderboard,
    Settings,
}
```

## 15.2 Home screen

The home screen contains:

1. **Play**
2. **Leaderboard**
3. **Settings**

Recommended presentation:

* Turfrace title centered or slightly left of center.
* A slowly animated white field contour behind the menu.
* Small decorative colored trails moving in the background.
* Last-used input device controls focus.
* Mouse hover and gamepad focus use the same highlight language.

There is no Quit button in the web build.

## 15.3 Join lobby

The lobby is the primary device-registration screen.

It contains:

* Up to eight human player cards.
* “Press any input to join.”
* A visible `Join with Mouse` control.
* Total competitor selector.
* NPC count preview.
* Start control.
* Back control.

### Joining

An unassigned device joins by:

* Pressing a gamepad face button or Start.
* Clicking `Join with Mouse`.
* Pressing a keyboard confirm or movement key.

A new player card receives:

* The most recently used unassigned profile, or
* A temporary `Player N` profile if none exists.
* The first unused preferred color.
* A default icon/pattern.

### Player card controls

Each player card permits:

* Cycle profile left/right.
* Select `New Profile`.
* Cycle color.
* Cycle icon/pattern.
* Toggle ready.
* Leave lobby.

Profiles already selected by another player are skipped.

Colors selected by humans must be unique. Cycling skips occupied colors.

### Shared navigation

Any joined player may navigate shared lobby controls.

A shared focus token is assigned to the device that most recently moved the global selection. Per-player card actions still apply only to the owning device.

### Starting

The standard game requires:

* At least two human players.
* Every joined player marked ready.
* The combined human and robot field does not exceed 12 competitors.

Any ready player may press Start.

A three-second countdown begins. Any player may cancel it with Back before the match loads.

## 15.4 Robot selector

Default robots: 0. Robots are added explicitly and the most recent robot preference is persisted.

The complete field remains limited to 2–12 competitors.

```text
total_competitors = joined_humans + npc_count
0 <= npc_count <= 12 - joined_humans
```

At least two joined humans are required to start.
If another human joins a full field, reduce the robot count to keep the total within 12.

NPC names, colors, patterns, and behavior styles are assigned when the match starts.

## 15.5 Profile creation

Selecting `New Profile` creates a persistent browser-local profile.

Required fields:

* Name, default `Player N`.
* Icon/pattern.
* Preferred color.

Name entry supports:

* Physical keyboard text entry.
* Mouse selection.
* A simple gamepad-navigable on-screen keyboard.

Names are limited to 16 displayed characters and sanitized for control characters.

## 15.6 In-game HUD

### Global HUD

A global UI camera renders:

* A compact top-three leaderboard in the top-right, using player-color rows and
  thin square accent rails without a panel background.
* Compact kill feed.
* Pause and game-over overlays.

Leaderboard row:

```text
[rank] [name] [claimed %] [color rail]
```

The live rows include the current territory percentage to one decimal place, so rank changes have
an immediately understandable cause. A thin color rail replaces detached square swatches. The rows
remain legible at narrow viewports and require no title or panel.

### Per-player HUD

Each human viewport contains:

* Player names projected above each world cube in that player's color.
* Respawn countdown when dead.
* Small spawn-protection timer or shield.
* Movement, trail, camera, and effect cues instead of duplicated numeric telemetry.

## 15.7 Pause screen

Any human may pause with Start/Menu.

Pause options:

* Resume.
* Settings.
* Reconnect Controllers.
* Replace Disconnected Player with NPC.
* Restart Match.
* Return to Lobby.

The simulation, NPC logic, timers, and capture animations freeze while paused.

Losing browser focus or hiding the page automatically pauses the match.

## 15.8 Results screen

Display:

* Winner profile and color.
* Match duration.
* Human placement.
* Peak territory percentage.
* Kills.
* Deaths.
* Largest single capture.
* Total cells captured.
* Longest active trail.
* `Rematch`.
* `Return to Lobby`.
* `Home`.

Rematch preserves:

* Human device assignments.
* Profiles.
* Colors where possible.
* Total competitor setting.

It generates a new field seed unless `Replay Same Field` is selected.

## 15.9 Local leaderboard screen

The home-screen leaderboard includes human profiles only.

Default sorting:

1. Wins.
2. Best territory percentage.
3. Kills.
4. Games played.

Columns:

* Rank.
* Profile name.
* Wins.
* Games.
* Win rate.
* Kills.
* Best territory.
* Largest capture.

NPC results are never persisted to this leaderboard.

---

# 16. Profiles and persistence

## 16.1 Storage

All data is browser-local.

Suggested keys:

```text
turfrace.profiles.v1
turfrace.settings.v1
turfrace.last_lobby.v1
turfrace.statistics.v1
```

Use JSON serialization with an explicit schema version.

If storage is unavailable:

* Continue with in-memory profiles.
* Show a non-blocking warning.
* Do not prevent play.

## 16.2 Profile schema

```rust
struct LocalProfile {
    schema_version: u32,
    id: String,
    display_name: String,
    icon_id: u8,
    preferred_color_id: u8,
    created_at_unix_ms: u64,
    last_used_at_unix_ms: u64,
    statistics: LifetimeStatistics,
}

struct LifetimeStatistics {
    games_played: u32,
    wins: u32,
    kills: u32,
    deaths: u32,
    total_captured_cells: u64,
    best_territory_percent: f32,
    largest_capture_percent: f32,
    longest_trail_world_units: f32,
}
```

## 16.3 Match statistics

Track per competitor:

```rust
struct MatchStatistics {
    kills: u32,
    deaths: u32,
    captures_completed: u32,
    cells_captured_total: u32,
    cells_stolen_total: u32,
    largest_capture_cells: u32,
    peak_territory_cells: u32,
    longest_trail_length: f32,
    time_alive_seconds: f32,
}
```

Only human profile statistics are persisted after results are confirmed.

## 16.4 Data controls

Settings must include:

* Rename profile.
* Delete profile.
* Reset leaderboard statistics.
* Reset all local data.

Destructive operations require confirmation.

---

# 17. NPC engine

## 17.1 Local-information runtime

NPCs use the same authoritative movement, trail, combat, death, capture, visibility, and respawn
rules as humans. The runtime pipeline is:

```text
local sensing → match memory → capture planner → tactical brain → skill/safety filter → SteeringIntent
```

Brains receive only a typed `NpcDecisionFrame`; they never receive `BoardGrid`, `TerritoryMap`,
arbitrary ECS queries, complete polygons, or historical trail vectors. The frame contains the
NPC's immediate movement state, public rank, at most three nearby rivals, at most two nearby trail
points, and an optional locally planned capture. Rivals and trails outside `14 + 14 × skill` world
units cannot change a decision.

The derived board cache maintains an owner-frontier bit mask as ownership changes. Home searches
are bounded frontier-cell searches, and local trail sensing reuses incremental segment buckets.
An ordinary thought performs no full-board, complete-polygon, or trail-history scan.

Every controller has fixed-size match memory for action commitment, four capture waypoints,
confidence, frustration, and per-opponent estimates. Personal
spawn, death, capture, kill, and theft events are delivered directly from authoritative match
resolution. Twenty percent of generated NPCs do not adapt; all memory resets between matches.

## 17.2 Capture planning and brains

While safely inside owned ground, the planner samples only locally visible owner-frontier cells.
It chooses a safe interior staging point, an exterior outbound apex, a laterally separated return
apex, and a distinct owned re-entry point. Staging before the boundary accounts for finite turn
rate and prevents accidental skim-out/skim-in loops. Desired depth and width scale with greed,
rank pressure, confidence, frustration, and nearby learned aggression. Plans persist across
thoughts; threat can shorten the return leg after the excursion is established but cannot cancel
the outbound leg immediately.

Two brains ship:

* `UtilityTactician`: the default planner-following, threat-aware brain.
* `LegacyWanderer`: a rare, deliberately less predictable compatibility character that still
  uses explicit capture plans.

Roster generation is deterministic from `npc_roster_seed`: a match has at most one legacy NPC,
with a 3% chance that the slot is present, and all remaining slots are tactical. Continuous traits are
skill, aggression, greed, exploration, composure, adaptability, commitment, and turning bias.
Unique names are shuffled independently and ordinary UI hides brain and traits.

## 17.3 Difficulty and imperfection

Lobby-wide difficulty samples skill from overlapping triangular distributions:

| Difficulty | Minimum | Mode | Maximum |
| ---------- | ------: | ---: | ------: |
| Easy       |    0.05 | 0.25 |    0.60 |
| Normal     |    0.25 | 0.55 |    0.85 |
| Hard       |    0.45 | 0.75 |    0.95 |

Think rate is `4 + 8 × skill` Hz, perception is `14 + 14 × skill` units, and slowly drifting
independent steering error interpolates from 14° to 1°. Lower skill reacts less often and commits
longer; commitment starts only when the selected action changes, so repeated thoughts cannot lock
the controller in its old action. Difficulty never changes speed, turn rate, collision, capture,
or visibility rules.

## 17.4 Seeds and reproducibility

`MatchSetup` records `field_seed`, `npc_roster_seed`, and `npc_difficulty`. A normal rematch advances
both seeds. **Replay Field** advances only the roster seed, producing new names, traits, and
reactions on identical terrain. Supplying both seeds reproduces the match exactly. Runtime play
performs no training, telemetry, network requests, or persistent player-behavior collection.

---

# 18. Visual specification

## 18.1 Overall style

The game resembles a clean tabletop model or animated paper canvas.

* Outside-field background: very pale neutral gray.
* Field surface: pure or nearly pure white.
* Field contour: soft gray line and subtle shadow.
* Territory: colorful, pastel-leaning, opaque surfaces.
* Trails: transparent saturated ribbons.
* Cubes: lighter variants of player colors with black outlines.
* UI: white, charcoal, and competitor accent colors.

## 18.2 Field rendering

The field surface sits at `Y = 0`.

The background outside the contour sits slightly below it, around `Y = -0.03`, creating a visible silhouette and subtle drop edge.

Recommended colors:

```text
Outside background: #F2F3F5
Field surface:      #FFFFFF
Field border:       #D5D8DE
Primary UI text:    #17191D
```

The field SHOULD have a very faint procedural dot or paper-grid shader at 2–3% opacity.

## 18.3 Territory rendering

Claimed territory top surface:

```text
Y = 0.17
```

Color:

```text
territory_color = mix(primary_color, white, 0.20)
```

Territory is opaque.

Rendering details:

* A dark antialiased rim suggests a shallow vertical side where claimed territory meets unclaimed field.
* Rim color is approximately 15% darker.
* Boundaries between two owners use a thin darkened seam rather than overlapping side walls.
* Ownership updates rebuild bounded smooth owner meshes with an elevated top and explicit side wall.
* Captures replace previous colors; territory is not stacked.

## 18.4 Trail rendering

Trails are flat ribbon meshes.

* Width: 0.65 units.
* Base alpha: 0.50.
* Material: unlit, premultiplied alpha.
* Corners: bevel or rounded joins.
* End cap: rounded behind the cube.
* Start edge: flush with the ownership boundary so the trail emerges from turf
  without a circular bulb.
* No vertical side walls.

Active trails use one stable traversal plane just above claimed territory:

* About `Y = 0.20` over unclaimed and claimed surfaces.
* The exact trail head is synchronized every presentation update, including
  between committed gameplay samples.
* A drawing cube remains at the claimed-territory traversal height until its
  trail closes.

This makes paths read as intentional bridges over every owner's territory,
avoids folded ribbon geometry at ownership boundaries, and prevents cubes from
snapping or clipping through the raised turf lip.

## 18.5 Cubes

Each player cube consists of:

1. A black outer cube at size 1.36.
2. A colored inner cube at size 1.20.
3. A small top-face profile symbol.
4. A flat translucent shadow underneath.

Inner color:

```text
cube_color = mix(primary_color, white, 0.30)
```

The cube:

* Rotates around Y to face movement direction.
* May lean up to 5° during sharp turns.
* Returns smoothly upright.
* Does not physically roll.

The top-face icon and territory pattern provide identification independent of color.

## 18.6 Recommended color palette

Human colors must be unique.

| ID | Name    | Hex       |
| -: | ------- | --------- |
|  1 | Blue    | `#2563EB` |
|  2 | Red     | `#DC2626` |
|  3 | Emerald | `#16A34A` |
|  4 | Purple  | `#7C3AED` |
|  5 | Orange  | `#EA580C` |
|  6 | Cyan    | `#0891B2` |
|  7 | Pink    | `#DB2777` |
|  8 | Amber   | `#D97706` |
|  9 | Teal    | `#0F766E` |
| 10 | Indigo  | `#4338CA` |
| 11 | Lime    | `#4D7C0F` |
| 12 | Brown   | `#854D0E` |

Every color is paired with a pattern such as:

* Solid.
* Diagonal lines.
* Reverse diagonal.
* Dots.
* Crosshatch.
* Horizontal lines.
* Vertical lines.
* Diamonds.
* Small squares.
* Waves.
* Rings.
* Chevrons.

## 18.7 Lightweight shader effects

### Required trail material

A custom trail material SHOULD add:

* A soft highlight band moving from trail origin toward the cube.
* Maximum brightness variation of approximately 8%.
* No gameplay-significant blinking.
* Alpha fixed around 50%.

The shader must avoid compute, storage buffers, and WebGPU-only features.

### Canvas shader

A procedural canvas shader MAY add:

* Fine dots or fibers.
* Very subtle world-space variation.
* No obvious repetition.
* No effect on gameplay readability.

### Danger pulse

When an enemy is close to a player’s trail:

* The trail’s alpha may pulse between 45% and 60%.
* The player’s viewport shows a warning.
* Reduced-motion mode replaces pulsing with a static outline.

## 18.8 Capture effect

On successful capture:

* Newly captured cells rise from 0 to 0.10 units over approximately 0.25 seconds, or
* A bright outline sweeps around the new boundary if height animation is too expensive.
* A short ring and bounded particle burst acknowledge the new boundary.
* Sound pitch scales mildly with capture size.

Authoritative ownership changes immediately. The animation is cosmetic.

## 18.9 Easy polish wins

The following SHOULD be prioritized because they are inexpensive and improve presentation:

* Blob shadows under cubes.
* Black cube outlines.
* Floating leader crown.
* Player-color name tags above cubes.
* Colored viewport accent line.
* Trail shimmer shader.
* Cube lean while turning.
* Small cube-fragment death burst.
* Edge warning glow.
* Subtle field shadow.
* Territory patterns for accessibility.
* Brief leader-change sound.
* Small home-direction arrow while drawing.

Avoid expensive baseline effects such as:

* Real-time shadows for every viewport.
* Screen-space ambient occlusion.
* Volumetric lighting.
* Heavy bloom.
* Per-camera reflections.
* Full-scene motion blur.

---

# 19. Audio specification

## 19.1 Required sounds

* Menu move.
* Menu confirm.
* Menu back.
* Player join.
* Ready/unready.
* Countdown ticks.
* `GO`.
* Trail start.
* Capture completion.
* Trail cut.
* Self-collision.
* Death.
* Respawn.
* Leader change.
* Victory.
* Pause/resume.

## 19.2 Mixing

Because all players share one physical screen and speaker system:

* Do not spatialize events relative to individual cameras.
* Events involving a human player should be slightly louder than NPC-only events.
* Multiple captures in one update must be rate-limited or mixed to avoid clipping.
* NPC-only trail-start sounds may be omitted.

## 19.3 Settings

* Master volume.
* Music volume.
* Effects volume.
* Mute when browser tab is unfocused.

Controller vibration MAY be used when available, but gameplay must never depend on it.

---

# 20. Accessibility and settings

## 20.1 Accessibility

Required options:

* Territory patterns on/off.
* Reduced motion.
* Camera shake intensity: Off, Low, Full.
* High-contrast UI.
* Adjustable gamepad deadzone.
* Adjustable mouse sensitivity.
* Larger HUD text.
* Master and category volume controls.

### Responsive UI sizing

Menus and overlays are authored against a `1280×720` logical reference canvas,
then rendered through one uniform scale derived from the smaller viewport axis.
This keeps typography and hit targets proportional on 16:9, 16:10, 4:3, and
ultrawide displays without stretching one axis. The scale is clamped to a
minimum of `1.0` for compact stress layouts and a maximum of `3.0` for large
desktop/Retina canvases. Bevy's logical window size is used, so a Retina device
does not accidentally double the UI merely because its backing framebuffer has
twice the pixel density.

New UI text should generally target these logical sizes:

* Captions and table metadata: `12–14px`.
* Body/status text and secondary actions: `16–18px`.
* Primary interactive labels: `18–22px`.
* Screen titles and major announcements: `42–68px`.

Every menu must remain usable at `200%` text/UI scaling: labels may wrap or
reflow, but content and interactive function must not be lost. This follows
the intent of WCAG 2.2 Success Criterion 1.4.4 while preserving a game-like
composition at the normal reference scale.

Reduced-motion mode disables:

* Cube leaning.
* Capture height waves.
* Strong camera easing.
* Pulsing trail warnings.
* Large screen shake.

## 20.2 Graphics settings

### Low

* No dynamic lighting shadows.
* No capture height animation.
* Static trail material.
* Reduced particles.
* Device-pixel-ratio cap of 1.0.

### Medium, default

* Blob shadows.
* Trail shimmer.
* Moderate particles.
* Capture boundary sweep.
* Device-pixel-ratio cap around 1.5.

### High

* More particles.
* Higher render scale.
* Optional contact-style detail where supported.
* Enhanced territory animation.
* Device-pixel-ratio cap around 2.0.

All modes retain identical gameplay information.

---

# 21. Bevy application architecture

## 21.1 Plugin layout

Recommended plugins:

```text
TurfraceAppPlugin
├── AppStatePlugin
├── ConfigurationPlugin
├── PersistencePlugin
├── ProfilePlugin
├── InputPlugin
├── LobbyPlugin
├── MatchPlugin
├── BoardPlugin
├── MovementPlugin
├── TrailPlugin
├── CapturePlugin
├── CombatPlugin
├── RespawnPlugin
├── RankingPlugin
├── NpcPlugin
├── SplitScreenPlugin
├── GameUiPlugin
├── TerritoryRenderPlugin
├── EffectsPlugin
├── AudioPlugin
├── WebIntegrationPlugin
└── DebugToolsPlugin
```

## 21.2 Suggested source layout

```text
src/
├── main.rs
├── app_state.rs
├── config.rs
├── ids.rs
├── profiles/
│   ├── mod.rs
│   ├── model.rs
│   └── storage.rs
├── input/
│   ├── mod.rs
│   ├── devices.rs
│   ├── gamepad.rs
│   ├── mouse.rs
│   └── steering.rs
├── lobby/
│   ├── mod.rs
│   ├── join.rs
│   └── ui.rs
├── match_game/
│   ├── mod.rs
│   ├── lifecycle.rs
│   └── statistics.rs
├── board/
│   ├── mod.rs
│   ├── generation.rs
│   ├── grid.rs
│   ├── distance_field.rs
│   └── spawning.rs
├── movement/
│   ├── mod.rs
│   └── boundary.rs
├── trail/
│   ├── mod.rs
│   ├── sampling.rs
│   ├── raster.rs
│   └── collision.rs
├── capture/
│   ├── mod.rs
│   ├── path.rs
│   ├── polygon.rs
│   └── ownership.rs
├── combat/
│   ├── mod.rs
│   └── death.rs
├── npc/
│   ├── mod.rs
│   ├── brain.rs
│   ├── perception.rs
│   ├── planner.rs
│   └── personalities.rs
├── camera/
│   ├── mod.rs
│   ├── layout.rs
│   └── follow.rs
├── render/
│   ├── mod.rs
│   ├── field.rs
│   ├── territory.rs
│   ├── trail.rs
│   ├── cube.rs
│   └── materials.rs
├── ui/
│   ├── mod.rs
│   ├── home.rs
│   ├── lobby.rs
│   ├── hud.rs
│   ├── results.rs
│   └── settings.rs
├── audio/
│   └── mod.rs
├── web/
│   ├── mod.rs
│   ├── storage.rs
│   └── fullscreen.rs
└── debug/
    ├── mod.rs
    ├── overlays.rs
    └── replay_log.rs
```

## 21.3 Core components

```rust
#[derive(Component)]
struct Competitor {
    id: CompetitorId,
    display_name: String,
    color_id: u8,
    pattern_id: u8,
}

#[derive(Component)]
enum CompetitorKind {
    Human { local_player_index: u8 },
    Npc { personality: NpcPersonality },
}

#[derive(Component)]
struct MotionState {
    position: Vec2,
    previous_position: Vec2,
    heading: Vec2,
    desired_heading: Vec2,
    speed: f32,
    max_turn_rate: f32,
}

#[derive(Component)]
enum LifeState {
    Alive {
        spawn_protection_remaining: f32,
    },
    Respawning {
        remaining: f32,
    },
}

#[derive(Component)]
struct TerritoryState {
    owned_cells: u32,
    peak_owned_cells: u32,
}

#[derive(Component)]
struct ActiveTrail {
    points: Vec<Vec2>,
    cells: Vec<CellIndex>,
    start_owned_cell: CellIndex,
    total_length: f32,
    segment_sequence: u32,
}

#[derive(Component)]
struct HumanInputBinding {
    device: InputDeviceId,
}

#[derive(Component)]
struct PlayerCamera {
    competitor: CompetitorId,
    viewport_slot: u8,
}
```

## 21.4 Core resources

```rust
struct GameConfig;
struct BoardGrid;
struct MatchSeed(u64);
struct MatchClock;
struct CompetitorRegistry;
struct DeviceRegistry;
struct RankingTable;
struct RespawnQueue;
struct NpcBrainRegistry;
struct LocalProfileStore;
struct UserSettings;
```

## 21.5 Important events

```rust
struct PlayerJoined;
struct PlayerLeft;
struct TrailStarted;
struct TrailClosed;
struct TrailCut;
struct CaptureResolved;
struct CompetitorKilled;
struct RespawnStarted;
struct CompetitorRespawned;
struct TerritoryChanged;
struct RankingChanged;
struct LeaderChanged;
struct MatchWon;
```

## 21.6 Physics decision

Do not use a general-purpose physics engine for core gameplay.

Movement and collision are two-dimensional and can be implemented more deterministically with:

* Grid field mask.
* Signed-distance boundary.
* Swept circles/capsules.
* Rasterized trail cells.

Rendered cubes are visual representations of logical movement, not rigid bodies.

---

# 22. Rendering implementation

## 22.1 Field mesh

Because the contour is star-shaped:

* Generate one center vertex.
* Generate 128 boundary vertices.
* Create a triangle fan.
* Add a thin vertical outer skirt for field depth.
* Add a soft shadow mesh below the field.

## 22.2 Territory chunks

Divide the ownership grid into 32 × 32-cell chunks.

Each chunk owns one dynamic mesh containing:

* Claimed cell top faces.
* Claimed-to-unclaimed side walls.
* Owner-change seam geometry.
* Per-vertex color and pattern coordinates.

When ownership changes:

* Mark affected chunks dirty.
* Include neighboring chunks when an ownership edge crosses a chunk boundary.
* Rebuild dirty meshes after gameplay simulation.
* Reuse mesh handles rather than respawning entities.

A standard eight-player field should require approximately 50–100 chunks depending on generated size.

## 22.3 Trail meshes

Each active trail is one dynamic ribbon mesh.

Rebuild only when new points are appended.

The visual ribbon may use a simplified point list, but collision must retain the authoritative raster coverage.

## 22.4 Cube rendering

Each logical competitor owns a render hierarchy:

```text
PlayerRoot
├── BlackOutlineCube
├── ColoredInnerCube
├── TopIcon
├── LeaderIndicator
├── SpawnShield
└── BlobShadow
```

Only `PlayerRoot` follows logical movement.

## 22.5 UI cameras

Use:

* One 3D camera per human player.
* A dedicated UI root targeted to each camera.
* One final high-order global UI camera for the leaderboard, kill feed, pause, and results overlays.

---

# 23. Static web application

## 23.1 Deployment contents

The build output contains only static files:

```text
index.html
turfrace.js
turfrace_bg.wasm
assets/
manifest.webmanifest        optional
service-worker.js           optional
```

No API server is required.

## 23.2 Browser shell

The HTML shell must:

* Fill the viewport.
* Center the canvas.
* Prevent page scrolling during play.
* Provide a loading indicator.
* Display a readable error if WebGL2 initialization fails.
* Provide a user-gesture start screen if audio initialization requires it.
* Support fullscreen from a menu action.
* Avoid intercepting browser refresh shortcuts unnecessarily.

## 23.3 Recommended web release processing

* Release-mode Rust build.
* Link-time optimization.
* Optimize WASM for size.
* Serve Brotli or gzip compression.
* Cache content-hashed assets.
* Do not cache `index.html` indefinitely.
* Keep profile data entirely in browser storage.
* Do not include telemetry in v0.1.

## 23.4 Optional offline support

A service worker and web manifest MAY make the game installable and playable offline after first load.

This is a useful enhancement for a static game but is not required for initial gameplay acceptance.

---

# 24. Performance requirements

## 24.1 Target

At 1920 × 1080 on a recent desktop or laptop browser:

* 60 FPS target with 2–8 human viewports.
* Simulation must remain at 60 fixed updates per second.
* Minimum acceptable sustained frame rate: 45 FPS on Medium.
* Low mode should remain playable at 30 FPS on weaker hardware.
* Input-to-visible-response target: no more than two rendered frames.
* Standard board ownership grid: under 100,000 cells.
* Memory target: under 256 MB after loading.

Although there may be up to eight cameras, their viewports partition one window, so total shaded pixel count remains near the window resolution. The major risk is repeated scene traversal and draw-call overhead, not eight full-resolution renders.

## 24.2 Performance safeguards

* Disable real-time shadow maps by default.
* Use blob shadows.
* Rebuild bounded territory meshes and upload ownership only when its revision changes.
* Pool particles.
* Keep NPC thinking below the fixed simulation rate.
* Use trail bitmasks for broad-phase collision.
* Smooth and triangulate territory boundaries during ownership updates; steady-state shading uses
  opaque owner meshes with no filtered alpha coverage.
* Cap browser device-pixel ratio.
* Hide or cull effects outside each camera.
* Avoid one entity per territory cell.
* Avoid per-cell Bevy components.
* Avoid allocating large vectors in fixed-update systems.
* Reuse scratch buffers for A*, scanline fill, and rasterization.

---

# 25. Testing requirements

## 25.1 Unit tests

Required tests include:

* World-position to cell conversion.
* Cell-center to world conversion.
* Field-mask generation.
* Field-mask connectivity.
* Signed-distance boundary direction.
* Trail capsule rasterization.
* Self-trail recent-segment exclusion.
* Swept trail collision.
* Owned-cell A* path.
* Polygon scanline fill.
* Loop capture.
* Bridge capture.
* Territory stealing.
* Territory-count updates.
* Displaced death.
* Respawn-delay sequence.
* Ranking tie-breaks.
* Exact fixed-point 95% victory threshold.

## 25.2 Property tests

For randomized field seeds and capture shapes:

* No outside-field cell is ever owned.
* A cell never has more than one owner.
* Sum of owner counts equals the number of owned cells in the grid.
* Dead players have no territory or active trail.
* Every active trail bit corresponds to a valid active competitor trail.
* Initial spawn territories do not overlap.
* Every generated field is connected.
* Every generated field has sufficient spawn capacity.
* Capture results are deterministic for the same board and trail.
* Capture cannot change cells outside the field.
* Every non-empty competitor territory contains its spawn anchor and has one connected outer island.

## 25.3 Integration tests

* Two gamepads join unique slots.
* One mouse joins exactly one slot.
* Eight gamepads produce eight human viewports.
* NPC count matches the explicit robot selection.
* All joined players can navigate global lobby controls.
* Profile selection skips profiles already in use.
* Settings and profiles survive reload.
* Controller disconnection pauses and allows reassignment.
* Window resizing recalculates all viewports.
* Browser focus loss pauses.
* Death clears all territory.
* First three deaths respawn after 5, 10, and 15 seconds.
* NPCs can complete captures and cut trails.
* Severing a non-anchor island removes it and kills a cube standing on that island.
* NPC soak test runs for at least one simulated hour without invalid state.
* Match cannot continue after exact full-field capture.

## 25.4 Debug tools

Development builds SHOULD expose toggles for:

* Ownership cell grid.
* Field mask.
* Signed-distance contours.
* Trail raster cells.
* Collision sweep.
* Spawn candidate scores.
* NPC state and targets.
* Camera viewport rectangles.
* FPS and fixed-update timing.
* Current match seed.
* Input device assignments.

A deterministic input log SHOULD record:

* Match seed.
* Initial configuration.
* Per-fixed-update steering inputs.
* Join and disconnect events.

This allows gameplay bugs to be replayed exactly.

---

# 26. Acceptance criteria for v0.1

The initial version is complete when all of the following are true:

1. Two to eight humans can join with unique gamepads, with one optional mouse player.
2. The lobby defaults to zero robots; robots are added explicitly.
3. The combined human and robot field can be configured from 2 to 12 competitors.
4. Every cube moves continuously and responds to heading input.
5. The generated field has a visibly rounded, irregular contour.
6. Leaving territory creates a visible 50%-opacity trail.
7. Returning to connected territory claims the enclosed polygon.
8. Returning without a valid closure path converts only the trail corridor.
9. Captures overwrite enemy ownership.
10. Touching an opponent’s active trail kills its owner.
11. Touching an old section of one’s own trail causes self-death.
12. Death removes all territory and active trail immediately.
13. Respawn delays follow 5, 10, 15, 20 seconds and onward for humans and NPCs.
14. Respawn positions are safe and grant a small starting territory.
15. All human players receive correctly sized split-screen cameras.
16. The global top-right leaderboard shows the top three competitors including NPCs.
17. Local profiles can be created, selected, and persisted.
18. Local lifetime statistics appear in the home leaderboard.
19. NPCs can expand, return, hunt trails, die, and respawn using normal game rules.
20. Exact ownership of all playable cells ends the match.
21. The app runs from static files under HTTPS.
22. Medium quality maintains the performance target without dynamic shadow maps.
23. Color and pattern together identify every competitor.
24. Browser reload does not erase profiles or settings.
25. No gameplay rule relies on a WebGPU-only feature.

---

# 27. Recommended implementation sequence

## Stage 1: Headless simulation

Implement:

* Board grid.
* Field generation.
* Movement.
* Boundary handling.
* Territory ownership.
* Trail sampling.
* Capture rasterization.
* Death and respawn.
* Victory.
* Automated tests.

Use simple keyboard steering and debug drawing.

## Stage 2: Combat correctness

Add:

* Swept trail collisions.
* Self-trail collisions.
* Simultaneous deaths.
* Territory displacement.
* Simultaneous captures.
* Deterministic event ordering.
* Replay logging.

Do not begin visual polish until these rules are stable.

## Stage 3: 3D presentation

Add:

* Field mesh.
* Territory chunk meshes.
* Trail ribbons.
* Outlined cubes.
* Angled cameras.
* Split-screen layout.
* Per-camera HUD roots.

## Stage 4: Local multiplayer shell

Add:

* Device registry.
* Gamepad joining.
* Mouse player.
* Lobby.
* Profiles.
* Color selection.
* Settings.
* Controller reconnection.
* Pause flow.

## Stage 5: NPCs

Add:

* NPC interface.
* Perception queries.
* Cautious, Balanced, Raider, and Greedy personalities.
* Normal difficulty.
* Long-running simulation tests.

## Stage 6: Presentation and release

Add:

* Home and results screens.
* Local leaderboard.
* Audio.
* Trail shader.
* Capture effects.
* Leader crown.
* Accessibility patterns.
* Web release optimization.
* Browser test matrix.
* Optional offline installation.

---

# 28. Locked gameplay decisions

These decisions remove ambiguity for implementation:

* The final respawn sequence is 5, 10, 15, 20 seconds and onward.
* The sequence is identical for humans and NPCs.
* The boundary is solid and non-lethal.
* Cubes do not physically collide with one another.
* Touching one’s own established trail is fatal.
* Trail collisions resolve before capture closures.
* Territory ownership is exact fixed-point vector-authoritative; the sample grid is derived.
* Movement remains continuous and visually smooth.
* A valid loop is closed using a path through currently owned territory.
* A closure without such a path claims only the trail corridor.
* Territory can be stolen immediately with no defensive delay.
* Losing all territory causes death.
* Capturing ground under another cube does not directly kill it.
* All competitors, including NPCs, participate in one ranking.
* The top-right leaderboard shows the top three overall.
* Human viewport count is based on human players, not total competitors.
* Victory uses the exact fixed-point 95% area threshold, not a rounded percentage.
* Victory is checked before respawns.
* The initial release is entirely local and static, with no backend.
* WebGL2 is the compatibility baseline.

[1]: https://bevy.org/news/bevy-0-19/ "https://bevy.org/news/bevy-0-19/"
[2]: https://developer.mozilla.org/en-US/docs/Web/API/Gamepad_API "https://developer.mozilla.org/en-US/docs/Web/API/Gamepad_API"
[3]: https://developer.mozilla.org/en-US/docs/Web/API/GamepadButton/pressed "https://developer.mozilla.org/en-US/docs/Web/API/GamepadButton/pressed"
