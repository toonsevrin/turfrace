# NPC behavior laboratory

The lab is a bounded diagnostic runner over the production `HeadlessMatch` and
`SimulationPlugin`. It does not contain a second rules loop, inject combat
results, or treat an action label as a successful outcome. The fixture is only
an initial ECS condition; movement, trails, collision, closure, capture,
respawn, ranking, and events remain authoritative production systems.

## Run

```sh
scripts/npc-lab --fixture return-race --ticks 1800
scripts/npc-lab replay target/npc-lab/return-race.json --verify
```

The example can also be called directly:

```sh
cargo run --no-default-features --example npc_lab -- \
  --fixture interception --difficulty normal --personality hunter \
  --ticks 1800 --trace target/npc-lab/interception.json \
  --svg target/npc-lab/interception.svg
```

NPC acceptance fixtures are `return-race`, `interception`,
`bait-disengagement`, `distracted-territory`, and `loop-capture`. The
`bridge-capture` fixture is retained as a topology diagnostic only: its initial
geometry does not author a bridge maneuver, so it is not an NPC acceptance
test. Seeds may be overridden with `--field-seed` and `--npc-seed` (decimal or `0x` hexadecimal).
Ticks are restricted to 1..7200. `--record` is an alias for `--trace`.

The JSON artifact contains the exact `MatchSpec`, fixture/setup versions, both
seeds, requested variant, bounded human puppet commands, per-tick snapshots and
events, actual NPC decision samples, bounded motion trajectories, synchronized
trajectory ticks and ownership samples, board identity, setup hash, and an
explicit `acceptance` object. Decision samples use a bounded ring of at most
`trace_capacity` samples per NPC (clamped to 512); this is a per-NPC bound, not
a total bound, and long runs can discard early decisions. The artifact format
and setup versions are 2 because these evidence semantics changed. Tactic kind, detail,
phase, sequence, and explicit transition predecessor are recorded separately
from the display label.
Each acceptance check has a requirement, observed event or trajectory evidence,
and pass/fail result. Tactic labels never satisfy an
outcome check by themselves. `--svg` is a renderer-independent overlay of the
recorded actual motion samples. It is diagnostic output, not a gameplay
renderer or a performance measurement.

A fixture command writes its artifact before returning a non-zero status when
acceptance fails. This preserves evidence for failed runs. `replay ...
--verify` replays every fixed tick and then reports the artifact's acceptance
checks; it also returns non-zero for an unaccepted artifact. Replay verification
is deterministic simulation verification, not a claim that a failed fixture
passed.

## Interpretation and limits

`unverified` is no longer used. Acceptance is explicit and event/trajectory
based. Acceptance is policy-specific: use a hunter for interception, a raider for theft,
and a builder for the return, disengagement, and loop fixtures. `baseline` preserves
the seeded roster policy and is not expected to satisfy every authored encounter. `NpcProfileAdapter` is the single compatibility seam for
authored Builder/Hunter/Raider configuration: it maps the requested policy
while leaving independently generated competence untouched. No fixture is
accepted from a decision label alone, and a passing replay only proves that the
same recorded snapshots and authoritative events were reproduced.
Acceptance matches the first maneuver: first trail/cut, first positive theft
or capture, and first return/abandonment. Travel and death checks stop at that
encounter's first terminal outcome, rather than counting later respawns or
cycles.

Fixture setup uses deterministic named points, validates arena and board-cell
membership, applies vector claims through `TerritoryMap::apply_claim`, updates
territory records/statistics/rankings, and rasterizes any active trail created
by the production tick. The setup hash includes every polygon contour
coordinate, not merely equal-area territory identities. A setup mismatch fails
rather than relocating a point. Replay rejects unsupported format/setup
versions, mismatched specs, incomplete bounded recordings, and the first
divergent fixed tick by comparing both snapshot and event records.

The headless command does not support `--overlay`: shell visual integration is a
separate follow-up in `visual_playtest`; use `--svg` for a lightweight view
without renderer coupling.

## Verified acceptance matrix

Final validation follows the safety repair passes: fixed-tick forecasts with
held steering and synthetic trail growth, unconditional self-safety, spatial
trail history, observed-body threat prediction, and valid exits from enemy land.

The five fixtures below passed at their default seeds, Normal difficulty, and
600 fixed ticks each. Every recording was replay-verified against all snapshots
and events. Artifacts are `target/npc-lab/<fixture>.{json,svg}`.

| Fixture | Explicit personality | First-maneuver evidence |
| --- | --- | --- |
| `return-race` | `builder` | Return at tick 170, surviving owned-ground re-entry |
| `interception` | `hunter` | Authoritative trail cut and credited kill at tick 203 |
| `bait-disengagement` | `builder` | Threat return at tick 159, surviving re-entry |
| `distracted-territory` | `raider` | Positive stolen-area capture at tick 264 |
| `loop-capture` | `builder` | Positive loop closure at tick 134, no prior death |

Use `scripts/npc-lab --fixture NAME --personality POLICY --ticks 600 --svg PATH`.
The `authored_encounters_produce_authoritative_first_maneuver_outcomes` test
runs this matrix. Passing these seeded encounters is not proof of human-perceived
character recognizability, browser performance, or universal tactical success.
The interception setup installs a rasterized trail before tick one, so its
initial snapshot—not a later respawn's `TrailStarted` event—starts the window.

## Remaining validation limits

Full bounded `scripts/feedback` and WebAssembly compilation pass. Native
software-rendered screenshot attempts timed out without producing a capture;
visual quality, browser runtime performance, and physical controllers remain
unverified. Spatial-reference storage and route forecasts have explicit caps,
but bucket traversal still depends on indexed trail density. No browser frame
budget or broad seed/difficulty success-rate claim follows from this matrix.

The bridge diagnostic may inform future fixture geometry, but its `loop_fill`
value is not an NPC verdict. Keep genuine interception and theft acceptance
checks strict; failed runs must remain visible rather than being relabeled.
