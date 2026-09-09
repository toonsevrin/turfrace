//! Small deterministic runner used by replays, tests, and server-like tools.

use bevy::{prelude::*, time::Fixed};

#[cfg(test)]
use crate::config::GameConfig;
use crate::movement::CompetitorMotion;

use super::lifecycle::start_simulation;
use super::replay::{CommandRejection, ComparableCompetitor, ComparableSnapshot, PendingCommands};
use super::{
    Competitor, LifeState, MatchReplay, MatchSession, MatchSpec, SimulationClock, SimulationPlugin,
    SteeringCommand, TerritoryRecord,
};

#[derive(Clone, Debug, PartialEq)]
pub struct TickOutput {
    pub snapshot: ComparableSnapshot,
    pub events: Vec<super::SimulationEvent>,
}

/// An ECS world with no shell plugins. Callers advance exactly one authoritative
/// fixed tick at a time, so render/update cadence cannot affect a replay.
pub struct HeadlessMatch {
    app: App,
}

fn hash_word(hash: &mut u64, word: u64) {
    *hash ^= word;
    *hash = hash.wrapping_mul(0x1000_0000_01b3);
}

fn hash_f32(hash: &mut u64, value: f32) {
    hash_word(hash, u64::from(value.to_bits()));
}

fn territory_fingerprint(map: &crate::territory_map::TerritoryMap) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for geometry in std::iter::once(map.arena()).chain(map.territories()) {
        for contour in geometry.contours() {
            hash_word(&mut hash, contour.len() as u64);
            for point in contour {
                hash_word(&mut hash, point.x as u32 as u64);
                hash_word(&mut hash, point.y as u32 as u64);
            }
        }
        hash_word(&mut hash, u64::MAX);
    }
    hash
}

fn trail_fingerprint(trail: &crate::trail::ActiveTrail) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    hash_word(&mut hash, trail.points.len() as u64);
    for point in trail
        .points
        .iter()
        .copied()
        .chain(std::iter::once(trail.head))
    {
        hash_f32(&mut hash, point.x);
        hash_f32(&mut hash, point.y);
    }
    hash_word(&mut hash, trail.cells.len() as u64);
    hash
}

fn npc_fingerprint(npc: &crate::npc::NpcController) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    hash_f32(&mut hash, npc.think_remaining);
    hash_f32(&mut hash, npc.steering_error);
    hash_f32(&mut hash, npc.steering_error_target);
    hash_word(&mut hash, u64::from(npc.error_epoch));
    hash_word(&mut hash, u64::from(npc.safety_override as u8));
    hash_word(&mut hash, npc.memory.decision_counter);
    hash_word(&mut hash, npc.memory.waypoint_count as u64);
    hash_word(&mut hash, npc.memory.waypoint_index as u64);
    hash_f32(&mut hash, npc.memory.commitment_remaining);
    hash_f32(&mut hash, npc.memory.confidence);
    hash_f32(&mut hash, npc.memory.frustration);
    hash_f32(&mut hash, npc.memory.active_plan_risk);
    hash_f32(&mut hash, npc.memory.planned_capture_area);
    for waypoint in npc.memory.waypoints {
        hash_f32(&mut hash, waypoint.x);
        hash_f32(&mut hash, waypoint.y);
    }
    hash
}

impl HeadlessMatch {
    pub fn new(spec: MatchSpec) -> Result<Self, super::SpecValidationError> {
        spec.validate()?;
        let mut app = App::new();
        app.insert_resource(spec.config.clone());
        app.add_plugins(SimulationPlugin);
        // Run Startup so the fixed clock is configured, then create the match.
        app.update();
        start_simulation(app.world_mut(), &spec);
        let generation = app.world().resource::<super::MatchGeneration>().0;
        app.world_mut().resource_mut::<super::PresentationReady>().0 = Some(generation);
        Ok(Self { app })
    }

    pub fn acknowledge_presentation(&mut self, generation: u64) {
        self.app
            .world_mut()
            .resource_mut::<super::PresentationReady>()
            .0 = Some(generation);
    }

    pub fn restart(&mut self, spec: MatchSpec) -> Result<(), super::SpecValidationError> {
        spec.validate()?;
        start_simulation(self.app.world_mut(), &spec);
        let generation = self.generation();
        self.acknowledge_presentation(generation);
        Ok(())
    }

    pub fn generation(&self) -> u64 {
        self.app.world().resource::<super::MatchGeneration>().0
    }

    pub fn tick(&self) -> u64 {
        self.app.world().resource::<SimulationClock>().0
    }

    pub fn enqueue(&mut self, command: SteeringCommand) -> Result<(), CommandRejection> {
        self.app
            .world_mut()
            .resource_mut::<PendingCommands>()
            .enqueue(command)
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.app
            .world_mut()
            .resource_mut::<super::SimulationPaused>()
            .0 = paused;
    }

    /// Returns the commands consumed so far, including their authoritative
    /// ticks. The recording is part of the simulation world and survives UI
    /// presentation changes.
    pub fn recording(&self) -> MatchReplay {
        self.app.world().resource::<MatchReplay>().clone()
    }

    /// Enqueues commands and advances one fixed tick. Tick zero is accepted
    /// only here as a shorthand for the next authoritative tick. The whole
    /// packet is validated before any command is queued.
    pub fn step<I>(&mut self, commands: I) -> Result<TickOutput, CommandRejection>
    where
        I: IntoIterator<Item = SteeringCommand>,
    {
        let next_tick = self.tick().saturating_add(1);
        let commands = commands
            .into_iter()
            .map(|mut command| {
                if command.tick == 0 {
                    command.tick = next_tick;
                }
                command
            })
            .collect::<Vec<_>>();
        self.app
            .world_mut()
            .resource_mut::<PendingCommands>()
            .enqueue_batch(commands)?;
        let dt = self.app.world().resource::<Time<Fixed>>().timestep();
        self.app
            .world_mut()
            .resource_mut::<Time<Fixed>>()
            .advance_by(dt);
        self.app.world_mut().run_schedule(FixedUpdate);
        let events = self
            .app
            .world_mut()
            .resource_mut::<super::SimulationEvents>()
            .drain()
            .collect::<Vec<_>>();
        Ok(TickOutput {
            snapshot: self.snapshot(),
            events,
        })
    }

    pub fn snapshot(&mut self) -> ComparableSnapshot {
        let world = self.app.world_mut();
        let (tick, phase, winner) = {
            let session = world.resource::<MatchSession>();
            (
                world.resource::<SimulationClock>().0,
                session.phase,
                session.winner,
            )
        };
        let territory_fingerprint =
            territory_fingerprint(world.resource::<crate::territory_map::TerritoryMap>());
        let mut query = world.query::<(
            &Competitor,
            &CompetitorMotion,
            &LifeState,
            &TerritoryRecord,
            &super::MatchStatistics,
            &super::SpawnProtection,
            Option<&crate::trail::ActiveTrail>,
            Option<&crate::npc::NpcController>,
        )>();
        let mut competitors = query
            .iter(world)
            .map(
                |(competitor, motion, life, territory, stats, protection, trail, npc)| {
                    ComparableCompetitor {
                        id: competitor.id.0,
                        position: motion.position.to_array(),
                        heading: motion.heading.to_array(),
                        alive: life.is_alive(),
                        territory_area: territory.current_area,
                        kills: stats.kills,
                        kill_streak: stats.kill_streak,
                        deaths: stats.deaths,
                        captures_completed: stats.captures_completed,
                        respawn_remaining: life.respawn_remaining,
                        spawn_protection_remaining: protection.remaining,
                        trail_length: trail.map_or(0.0, |trail| trail.length),
                        trail_fingerprint: trail.map_or(0, trail_fingerprint),
                        npc_fingerprint: npc.map_or(0, npc_fingerprint),
                    }
                },
            )
            .collect::<Vec<_>>();
        competitors.sort_by_key(|competitor| competitor.id);
        ComparableSnapshot {
            tick,
            phase: format!("{:?}", phase),
            winner: winner.map(|winner| winner.0),
            competitors,
            territory_fingerprint,
        }
    }
}

impl From<HeadlessMatch> for App {
    fn from(headless: HeadlessMatch) -> Self {
        headless.app
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_game::RosterDescriptor;

    fn spec() -> MatchSpec {
        let config = GameConfig::default();
        MatchSpec::from_config(
            42,
            vec![RosterDescriptor::Npc, RosterDescriptor::Npc],
            &config,
        )
    }

    #[test]
    fn same_replay_produces_same_snapshots() {
        let commands = (1..=8)
            .map(|tick| SteeringCommand::new(tick, crate::ids::CompetitorId(0), Vec2::X, 1.0))
            .collect::<Vec<_>>();
        let mut a_spec = spec();
        a_spec.countdown_ticks = 0;
        a_spec.roster[0] = RosterDescriptor::Human {
            identity: "a".into(),
            display_name: "A".into(),
            color_id: 0,
            pattern_id: 0,
        };
        let mut a = HeadlessMatch::new(a_spec.clone()).unwrap();
        let mut b = HeadlessMatch::new(a_spec).unwrap();
        for command in commands {
            let left = a.step([command]).unwrap();
            let right = b.step([command]).unwrap();
            assert_eq!(left.snapshot, right.snapshot);
            assert_eq!(left.events, right.events);
        }
        let replay = a.recording();
        replay.validate_commands().unwrap();
        let encoded = replay.to_json().unwrap();
        let decoded = MatchReplay::from_json(&encoded).unwrap();
        let mut replayed = HeadlessMatch::new(decoded.spec.clone()).unwrap();
        let mut final_output = None;
        for command in decoded.commands.iter().copied() {
            final_output = Some(replayed.step([command]).unwrap());
        }
        assert_eq!(final_output.unwrap().snapshot, a.snapshot());
    }

    #[test]
    fn replay_json_round_trips() {
        let replay = MatchReplay::new(spec(), Vec::new());
        assert_eq!(
            MatchReplay::from_json(&replay.to_json().unwrap()).unwrap(),
            replay
        );
    }

    #[test]
    fn stale_readiness_and_restart_cannot_leak_state() {
        let mut match_ = spec();
        match_.countdown_ticks = 0;
        match_.roster[0] = RosterDescriptor::Human {
            identity: "a".into(),
            display_name: "A".into(),
            color_id: 0,
            pattern_id: 0,
        };
        let mut runner = HeadlessMatch::new(match_.clone()).unwrap();
        let old_generation = runner.generation();
        runner
            .step([SteeringCommand::new(
                0,
                crate::ids::CompetitorId(0),
                Vec2::X,
                1.0,
            )])
            .unwrap();
        runner.restart(match_).unwrap();
        let new_generation = runner.generation();
        assert_ne!(old_generation, new_generation);
        runner.acknowledge_presentation(old_generation);
        runner.step(std::iter::empty()).unwrap();
        assert_eq!(runner.tick(), 0);
        runner.acknowledge_presentation(new_generation);
        runner.step(std::iter::empty()).unwrap();
        assert_eq!(runner.tick(), 1);
        assert!(runner.recording().commands.is_empty());
    }

    #[test]
    fn rejected_batch_is_atomic_and_live_recording_round_trips() {
        let mut match_ = spec();
        match_.countdown_ticks = 0;
        match_.roster[0] = RosterDescriptor::Human {
            identity: "a".into(),
            display_name: "A".into(),
            color_id: 0,
            pattern_id: 0,
        };
        let mut runner = HeadlessMatch::new(match_).unwrap();
        let valid = SteeringCommand::new(0, crate::ids::CompetitorId(0), Vec2::X, 1.0);
        let invalid = SteeringCommand::new(0, crate::ids::CompetitorId(0), Vec2::X, 2.0);
        assert!(runner.step([valid, invalid]).is_err());
        let output = runner.step([valid]).unwrap();
        assert_eq!(output.snapshot.tick, 1);
        let replay = runner.recording();
        assert_eq!(replay.commands.len(), 1);
        assert_eq!(
            MatchReplay::from_json(&replay.to_json().unwrap()).unwrap(),
            replay
        );
    }
}
