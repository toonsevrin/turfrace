# Deterministic simulation

The default `shell` Cargo feature adds rendering, windows, devices, UI and audio.
The authoritative simulation also builds without it:

```sh
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo test --no-default-features --lib
```

`match_game::SimulationPlugin` installs the simulation schedules and resources;
`MatchPlugin` adds the interactive shell adapters. `HeadlessMatch` owns a small
Bevy app without rendering or device plugins:

```rust
use turfrace::{config::GameConfig, match_game::{HeadlessMatch, MatchSpec, RosterDescriptor}};

let mut spec = MatchSpec::from_config(42, vec![RosterDescriptor::Npc; 4], &GameConfig::default());
spec.countdown_ticks = 0;
let mut game = HeadlessMatch::new(spec).unwrap();
let output = game.step([]).unwrap();
assert_eq!(output.snapshot.tick, 1);
```

## Commands and replay

A `MatchSpec` serializes the field/NPC seeds, roster, configuration, rules and
countdown ticks. `SteeringCommand` carries a competitor, tick and steering intent.
Commands target human roster entries, start at tick 1, and are validated before
batch admission. Sparse steering persists until replaced. `HeadlessMatch::step`
also accepts tick 0 as shorthand for the next gameplay tick; stored recordings
use explicit ticks. `recording()` returns the consumed command stream and spec.

Replay determinism is scoped to the same build/platform, not cross-platform
bitwise floating-point compatibility. Snapshots include geometry, trail and NPC
fingerprints as well as competitor state; tick outputs also expose events.

## Lifecycle

Starting a match resets transient state and advances `MatchGeneration`. Loading
waits for `PresentationReady(Some(current_generation))`; stale acknowledgements
cannot release a new match. The graphical adapter checks current scene/view and
material pipeline readiness. The headless runner explicitly acknowledges its own
generation. Countdown and gameplay advance through fixed schedules, independent
of screen navigation; `SimulationPaused` suspends progression. `MatchRules`
controls victory, scoring and respawning. Disabled respawn produces an explicit
eliminated state, not an infinite timer.

## Territory ownership

`TerritoryMap` owns private polygon geometry, its spatial index, revisions and
sampled ownership/frontier caches. Seed, clear and claim operations update them
together. Geometry accessors are read-only; callers must not separately refresh
caches. Polygon area is authoritative for statistics and victory. `BoardGrid`
retains static arena sampling and trail collision caches, not mutable ownership.
