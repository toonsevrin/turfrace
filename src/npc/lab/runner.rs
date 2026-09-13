use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::{
    config::GameConfig,
    match_game::{Competitor, HeadlessMatch, MatchRules, MatchSpec, SteeringCommand},
    movement::CompetitorMotion,
    territory_map::TerritoryMap,
};

use super::{
    acceptance::evaluate_acceptance,
    fixtures::{
        FixtureSetup, apply_profile, install_fixture, npc_ids, puppet_commands, roster_for,
    },
    model::{
        EncounterFixture, LAB_FORMAT_VERSION, LAB_SETUP_VERSION, LabArtifact, LabDecisionSample,
        LabError, LabManeuverStats, LabReplay, LabReport, LabTrace, LabVariant, MAX_TICKS,
        MAX_TRACE_RECORDS, MAX_TRAJECTORY_POINTS, ReplayTick,
    },
    record::{event_name, record_event, tactic_detail, tactic_kind, tactic_label, tactic_phase},
};

pub struct LabRunner {
    headless: HeadlessMatch,
    fixture: EncounterFixture,
    variant: LabVariant,
    spec: MatchSpec,
    setup: FixtureSetup,
    ticks_limit: u64,
    commands: Vec<SteeringCommand>,
    expected: Vec<ReplayTick>,
    stats: LabManeuverStats,
    decisions: Vec<LabDecisionSample>,
    trajectories: BTreeMap<u8, Vec<[f32; 2]>>,
    trajectory_ownership: BTreeMap<u8, Vec<bool>>,
    trajectory_ticks: BTreeMap<u8, Vec<u64>>,
    last_tactic: BTreeMap<u8, (u64, Option<String>)>,
    sample_stride: u64,
}

impl LabRunner {
    pub fn new(
        fixture: EncounterFixture,
        field_seed: u64,
        npc_roster_seed: u64,
        mut variant: LabVariant,
        ticks: u64,
    ) -> Result<Self, LabError> {
        if ticks == 0 || ticks > MAX_TICKS {
            return Err(LabError::InvalidTicks);
        }
        variant.trace_capacity = variant.trace_capacity.clamp(1, MAX_TRACE_RECORDS);
        let roster = roster_for(fixture);
        let mut spec = MatchSpec::from_config(field_seed, roster, &GameConfig::default());
        spec.npc_roster_seed = npc_roster_seed;
        spec.npc_difficulty = variant.difficulty;
        spec.countdown_ticks = 0;
        spec.rules = MatchRules {
            victory_enabled: false,
            ..MatchRules::from(&spec.config)
        };
        let mut headless = HeadlessMatch::new(spec.clone()).map_err(|error| {
            LabError::InvalidSetup(format!("invalid generated spec: {error:?}"))
        })?;
        apply_profile(headless.world_mut(), variant.personality);
        let setup = install_fixture(headless.world_mut(), fixture, &spec)?;
        let npc_ids = npc_ids(headless.world_mut());
        let trajectories = npc_ids
            .iter()
            .copied()
            .map(|id| (id, Vec::new()))
            .collect::<BTreeMap<_, _>>();
        let trajectory_ownership = npc_ids
            .iter()
            .copied()
            .map(|id| (id, Vec::new()))
            .collect::<BTreeMap<_, _>>();
        let trajectory_ticks = npc_ids
            .into_iter()
            .map(|id| (id, Vec::new()))
            .collect::<BTreeMap<_, _>>();
        Ok(Self {
            headless,
            fixture,
            variant,
            spec,
            setup,
            ticks_limit: ticks,
            commands: Vec::new(),
            expected: Vec::new(),
            stats: LabManeuverStats::default(),
            decisions: Vec::new(),
            trajectories,
            trajectory_ownership,
            trajectory_ticks,
            last_tactic: BTreeMap::new(),
            sample_stride: (ticks / MAX_TRAJECTORY_POINTS as u64).max(1),
        })
    }

    pub fn run(mut self) -> Result<LabArtifact, LabError> {
        for tick in 1..=self.ticks_limit {
            let commands = puppet_commands(self.fixture, tick, &self.spec);
            self.commands.extend(commands.iter().copied());
            let replay_tick = self.step(commands, tick)?;
            self.expected.push(replay_tick);
        }
        let final_snapshot = self.headless.snapshot();
        let trace = LabTrace {
            decisions: self.decisions,
            trajectories: self.trajectories,
            trajectory_ticks: self.trajectory_ticks,
            trajectory_ownership: self.trajectory_ownership,
        };
        let acceptance = evaluate_acceptance(self.fixture, &self.expected, &trace, &self.stats);
        let replay = LabReplay {
            format_version: LAB_FORMAT_VERSION,
            setup_version: LAB_SETUP_VERSION,
            fixture: self.fixture,
            field_seed: self.spec.seed,
            npc_roster_seed: self.spec.npc_roster_seed,
            variant: self.variant.clone(),
            board_generation: self.setup.board_generation,
            setup_hash: self.setup.setup_hash,
            spec: self.spec.clone(),
            human_commands: self.commands,
            ticks: self.ticks_limit,
            expected: self.expected,
        };
        Ok(LabArtifact {
            replay,
            report: LabReport {
                format_version: LAB_FORMAT_VERSION,
                setup_version: LAB_SETUP_VERSION,
                fixture: self.fixture,
                field_seed: self.spec.seed,
                npc_roster_seed: self.spec.npc_roster_seed,
                variant: self.variant,
                board_generation: self.setup.board_generation,
                setup_hash: self.setup.setup_hash,
                final_snapshot,
                stats: self.stats,
                trace,
                acceptance,
            },
        })
    }

    pub(crate) fn step(
        &mut self,
        commands: Vec<SteeringCommand>,
        tick: u64,
    ) -> Result<ReplayTick, LabError> {
        let output = self
            .headless
            .step(commands)
            .map_err(|error| LabError::Command(format!("tick {tick}: {error:?}")))?;
        self.observe(tick, &output.events);
        Ok(ReplayTick {
            snapshot: output.snapshot,
            events: output.events.iter().map(record_event).collect(),
        })
    }

    pub(crate) fn spec(&self) -> &MatchSpec {
        &self.spec
    }

    pub(crate) fn setup_identity(&self) -> (u64, u64) {
        (self.setup.board_generation, self.setup.setup_hash)
    }

    fn observe(&mut self, tick: u64, events: &[crate::match_game::SimulationEvent]) {
        self.stats.fixed_ticks = tick;
        for event in events {
            let key = event_name(event).to_owned();
            *self.stats.event_counts.entry(key).or_default() += 1;
            match event {
                crate::match_game::SimulationEvent::Capture { .. } => self.stats.captures += 1,
                crate::match_game::SimulationEvent::Death { .. } => self.stats.deaths += 1,
                crate::match_game::SimulationEvent::Kill { .. } => self.stats.kills += 1,
                crate::match_game::SimulationEvent::TrailStarted { .. } => {
                    self.stats.trail_starts += 1
                }
                _ => {}
            }
        }
        let world = self.headless.world_mut();
        let config = world.resource::<GameConfig>().clone();
        let map = world.resource::<TerritoryMap>().clone();
        let mut query = world.query::<(
            &Competitor,
            &CompetitorMotion,
            Option<&crate::npc::NpcController>,
        )>();
        for (competitor, motion, controller) in query.iter(world) {
            let id = competitor.id.0;
            let distance = motion.position.distance(motion.previous_position);
            *self.stats.path_length.entry(id).or_default() += distance;
            if map.arena_signed_distance(motion.position) <= config.collision_radius + 0.03 {
                *self.stats.boundary_contacts.entry(id).or_default() += 1;
            }
            if let Some(controller) = controller {
                self.stats
                    .max_steering_error
                    .entry(id)
                    .and_modify(|error| *error = error.max(controller.steering_error.abs()))
                    .or_insert(controller.steering_error.abs());
                let (tactic_kind, tactic_detail, tactic_phase, tactic_started_tick) = controller
                    .tactic
                    .map_or((None, None, None, None), |tactic| {
                        (
                            Some(tactic_kind(tactic.kind).to_owned()),
                            tactic_detail(tactic.kind),
                            Some(tactic_phase(tactic.phase).to_owned()),
                            Some(tactic.started_tick),
                        )
                    });
                let previous_tactic_kind =
                    self.last_tactic.get(&id).and_then(|(_, kind)| kind.clone());
                let tactic_transition =
                    self.last_tactic
                        .get(&id)
                        .is_none_or(|(sequence, previous_kind)| {
                            *sequence != controller.tactic_sequence || *previous_kind != tactic_kind
                        });
                self.last_tactic
                    .insert(id, (controller.tactic_sequence, tactic_kind.clone()));
                let sample = LabDecisionSample {
                    tick,
                    npc: id,
                    action: controller
                        .tactic
                        .map_or_else(|| "none".to_owned(), |tactic| tactic_label(tactic.kind)),
                    reason: controller.last_reason.to_owned(),
                    tactic_kind,
                    tactic_detail,
                    tactic_phase,
                    tactic_sequence: controller.tactic_sequence,
                    tactic_started_tick,
                    tactic_transition,
                    previous_tactic_kind,
                    position: motion.position.to_array(),
                    heading: motion.heading.to_array(),
                    steering_error: controller.steering_error,
                    safety_override: controller.safety_override,
                };
                // This is a per-NPC ring, not a first-N decision log. Later
                // returns, abandons, and post-event transitions must remain
                // inspectable when a fixture runs longer than the cap.
                let npc_samples = self
                    .decisions
                    .iter()
                    .filter(|decision| decision.npc == id)
                    .count();
                if npc_samples >= self.variant.trace_capacity
                    && let Some(index) = self
                        .decisions
                        .iter()
                        .position(|decision| decision.npc == id)
                {
                    self.decisions.remove(index);
                }
                self.decisions.push(sample);
                if (tick.is_multiple_of(self.sample_stride) || tick == self.ticks_limit)
                    && let Some(points) = self.trajectories.get_mut(&id)
                    && points.len() < MAX_TRAJECTORY_POINTS
                {
                    points.push(motion.position.to_array());
                    if let Some(ownership) = self.trajectory_ownership.get_mut(&id) {
                        ownership.push(map.owns(motion.position, competitor.id));
                    }
                    if let Some(ticks) = self.trajectory_ticks.get_mut(&id) {
                        ticks.push(tick);
                    }
                }
            }
        }
    }

    pub fn into_app(self) -> App {
        self.headless.into()
    }
}
