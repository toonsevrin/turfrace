//! Lifecycle for the authoritative, device-free simulation.
//!
//! Starting a simulation is a world operation so it can be used by the normal
//! shell and by the headless replay runner. No application state, lobby, input
//! device, or presentation type is used here.

use bevy::{prelude::*, time::Fixed};

use crate::{
    board::{BoardGrid, choose_initial_spawns},
    config::GameConfig,
    ids::CompetitorId,
    movement::CompetitorMotion,
    npc::{
        NpcController, NpcEvent, NpcEventMessage, NpcEventQueue, NpcRosterEntry,
        generate_npc_roster,
    },
    territory_map::TerritoryMap,
};

use super::replay::{MatchReplay, PendingCommands};
use super::{
    Competitor, CompetitorKind, DisplacementCredits, EliminationFeed, LastOwnedCell, LifeState,
    MatchGeneration, MatchPhase, MatchSession, MatchSpec, MatchStatistics, PendingCaptures,
    PendingDeaths, PresentationReady, Rankings, RosterDescriptor, SimulationClock,
    SimulationEvents, SimulationPaused, SpawnProtection, SteeringIntent, TerritoryRecord,
};

/// Fully resets and starts a simulation from a serializable specification.
/// Starting a match is deliberately the only place that advances its
/// generation and clears transient state.
pub fn start_simulation(world: &mut World, spec: &MatchSpec) {
    if let Err(error) = spec.validate() {
        panic!("invalid MatchSpec: {error:?}");
    }
    // The specification, rather than a mutable shell resource, is the
    // authoritative configuration for this run.
    world.insert_resource(spec.config.clone());
    if !world.contains_resource::<SimulationEvents>() {
        world.insert_resource(SimulationEvents::default());
    }
    if !world.contains_resource::<Rankings>() {
        world.insert_resource(Rankings::default());
    }
    if !world.contains_resource::<EliminationFeed>() {
        world.insert_resource(EliminationFeed::default());
    }
    if !world.contains_resource::<PendingDeaths>() {
        world.insert_resource(PendingDeaths::default());
    }
    if !world.contains_resource::<PendingCaptures>() {
        world.insert_resource(PendingCaptures::default());
    }
    if !world.contains_resource::<DisplacementCredits>() {
        world.insert_resource(DisplacementCredits::default());
    }
    if !world.contains_resource::<SimulationClock>() {
        world.insert_resource(SimulationClock::default());
    }
    if !world.contains_resource::<PendingCommands>() {
        world.insert_resource(PendingCommands::default());
    }
    if !world.contains_resource::<MatchGeneration>() {
        world.insert_resource(MatchGeneration::default());
    }
    if !world.contains_resource::<PresentationReady>() {
        world.insert_resource(PresentationReady::default());
    }
    if !world.contains_resource::<crate::match_game::rules::MatchRules>() {
        world.insert_resource(spec.rules.clone());
    }
    if !world.contains_resource::<NpcEventQueue>() {
        world.insert_resource(NpcEventQueue::default());
    }
    world.init_resource::<SimulationPaused>();
    reset_transient_resources(world);
    despawn_competitors(world);

    let config = world.resource::<GameConfig>().clone();
    // Restarting also resets fixed-time accumulation. Otherwise a restart
    // can inherit a partial old timestep and consume an extra tick.
    world.insert_resource(Time::<Fixed>::from_hz(config.fixed_hz));
    let roster = spec.roster.clone();
    let count = roster.len();
    let board = BoardGrid::generate(spec.seed, count, &config);
    let mut territory = TerritoryMap::from_board(&board);
    let spawns = choose_initial_spawns(&board, count, 8.0, spec.seed);
    for (index, &spawn) in spawns.iter().enumerate() {
        territory.seed_owner(
            spawn,
            config.starting_territory_radius,
            CompetitorId(index as u8),
        );
    }
    let countdown_remaining = spec.countdown_ticks as f32 / config.fixed_hz as f32;

    world.insert_resource(board);
    world.insert_resource(territory);
    world.insert_resource(MatchSession {
        field_seed: spec.seed,
        npc_roster_seed: spec.npc_roster_seed,
        purpose: spec.purpose,
        phase: MatchPhase::Loading,
        elapsed_seconds: 0.0,
        countdown_remaining,
        countdown_ticks_remaining: spec.countdown_ticks,
        winner: None,
        result_hold_remaining: 0.0,
    });
    world.insert_resource(spec.clone());
    world.insert_resource(spec.rules.clone());
    world.insert_resource(MatchReplay::new(spec.clone(), Vec::new()));
    world.insert_resource(Rankings::default());
    world.resource_mut::<SimulationPaused>().0 = false;
    world
        .resource_mut::<PendingCommands>()
        .set_controlled_players(&roster);
    world.resource_mut::<SimulationEvents>().0.clear();
    world.resource_mut::<NpcEventQueue>().0.clear();
    world.insert_resource(EliminationFeed::default());
    world.resource_mut::<SimulationClock>().0 = 0;
    let generation = world.resource::<MatchGeneration>().0.wrapping_add(1);
    world.resource_mut::<MatchGeneration>().0 = generation;
    world.resource_mut::<PresentationReady>().0 = None;

    let npc_count = roster
        .iter()
        .filter(|entry| matches!(entry, RosterDescriptor::Npc))
        .count();
    let npc_roster = generate_npc_roster(spec.npc_roster_seed, npc_count, spec.npc_difficulty);
    let mut next_npc = 0;
    for (index, (descriptor, &position)) in roster.iter().zip(spawns.iter()).enumerate() {
        let npc_entry = if matches!(descriptor, RosterDescriptor::Npc) {
            let entry = npc_roster.get(next_npc);
            next_npc += 1;
            entry
        } else {
            None
        };
        let territory_area = world
            .resource::<TerritoryMap>()
            .area(CompetitorId(index as u8));
        spawn_competitor(
            world,
            &config,
            territory_area,
            index,
            position,
            descriptor,
            npc_entry,
        );
        if matches!(descriptor, RosterDescriptor::Npc) {
            let tick = world.resource::<super::model::SimulationClock>().0;
            world
                .resource_mut::<NpcEventQueue>()
                .0
                .push(NpcEventMessage {
                    recipient: CompetitorId(index as u8),
                    event: NpcEvent::Spawned,
                    tick,
                });
        }
    }
}

fn reset_transient_resources(world: &mut World) {
    if let Some(mut queue) = world.get_resource_mut::<NpcEventQueue>() {
        queue.0.clear();
    }
    if let Some(mut events) = world.get_resource_mut::<SimulationEvents>() {
        events.0.clear();
    }
    if let Some(mut feed) = world.get_resource_mut::<EliminationFeed>() {
        feed.0.clear();
    }
    if let Some(mut pending) = world.get_resource_mut::<PendingDeaths>() {
        pending.0.clear();
    }
    if let Some(mut pending) = world.get_resource_mut::<PendingCaptures>() {
        pending.0.clear();
    }
    if let Some(mut credits) = world.get_resource_mut::<DisplacementCredits>() {
        credits.0.clear();
    }
    if let Some(mut clock) = world.get_resource_mut::<SimulationClock>() {
        clock.0 = 0;
    }
    if let Some(mut commands) = world.get_resource_mut::<PendingCommands>() {
        commands.reset();
    }
    if let Some(mut paused) = world.get_resource_mut::<SimulationPaused>() {
        paused.0 = false;
    }
}

fn spawn_competitor(
    world: &mut World,
    config: &GameConfig,
    territory_area: f32,
    index: usize,
    position: Vec2,
    descriptor: &RosterDescriptor,
    npc_entry: Option<&NpcRosterEntry>,
) {
    let id = CompetitorId(index as u8);
    let heading = (-position).try_normalize().unwrap_or(Vec2::Y);
    let (kind, display_name, color_id, pattern_id) = match descriptor {
        RosterDescriptor::Human {
            display_name,
            color_id,
            pattern_id,
            ..
        } => (
            CompetitorKind::Human,
            display_name.clone(),
            *color_id,
            *pattern_id,
        ),
        RosterDescriptor::Npc => {
            let entry = npc_entry.expect("every NPC slot has a deterministic roster entry");
            (
                CompetitorKind::Npc,
                entry.name.clone(),
                index as u8,
                (index % 8) as u8,
            )
        }
    };
    world.spawn((
        id,
        Competitor {
            id,
            display_name,
            kind,
            color_id,
            pattern_id,
        },
        CompetitorMotion::new(position, heading),
        LifeState::alive(),
        SpawnProtection {
            remaining: config.spawn_protection_seconds,
            elapsed: 0.0,
        },
        TerritoryRecord {
            current_area: territory_area,
            peak_area: territory_area,
        },
        MatchStatistics {
            peak_territory_area: territory_area,
            ..default()
        },
        LastOwnedCell(
            world
                .resource::<BoardGrid>()
                .world_to_cell(position)
                .unwrap(),
        ),
        SteeringIntent {
            desired_direction: heading,
            magnitude: 0.0,
            source: super::ControlSource::Npc,
        },
    ));
    if kind == CompetitorKind::Npc {
        let entry = npc_entry.expect("NPC descriptor without roster entry");
        // Insert separately so the base entity remains device-free and the
        // controller is still an ordinary simulation component.
        let entity = {
            let mut query = world.query::<(Entity, &Competitor)>();
            query
                .iter(world)
                .find(|(_, competitor)| competitor.id == id)
                .map(|(entity, _)| entity)
                .expect("new competitor entity exists")
        };
        world
            .entity_mut(entity)
            .insert(NpcController::from_roster(id, entry, index));
    }
}

fn despawn_competitors(world: &mut World) {
    let entities: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<Competitor>>();
        query.iter(world).collect()
    };
    for entity in entities {
        world.despawn(entity);
    }
}

/// Loading is released only by an acknowledgement for this exact match
/// generation. This keeps an acknowledgement from a previous restart inert.
pub(super) fn transition_when_ready(
    generation: Res<MatchGeneration>,
    ready: Res<PresentationReady>,
    paused: Res<SimulationPaused>,
    mut session: ResMut<MatchSession>,
    mut events: ResMut<SimulationEvents>,
) {
    if paused.0 || session.phase != MatchPhase::Loading || ready.0 != Some(generation.0) {
        return;
    }
    if session.countdown_ticks_remaining == 0 {
        session.phase = MatchPhase::Running;
        events.0.push(super::SimulationEvent::Go);
    } else {
        session.phase = MatchPhase::Countdown;
    }
}

/// Countdown advances by exactly one authoritative fixed update, rather than
/// subtracting a float and hoping it lands on zero.
pub(super) fn advance_countdown(
    config: Res<GameConfig>,
    paused: Res<SimulationPaused>,
    mut session: ResMut<MatchSession>,
    mut events: ResMut<SimulationEvents>,
) {
    if paused.0 || session.phase != MatchPhase::Countdown {
        return;
    }
    let before = (session.countdown_ticks_remaining as f32 / config.fixed_hz as f32).ceil() as u8;
    session.countdown_ticks_remaining = session.countdown_ticks_remaining.saturating_sub(1);
    session.countdown_remaining = session.countdown_ticks_remaining as f32 / config.fixed_hz as f32;
    let after = (session.countdown_ticks_remaining as f32 / config.fixed_hz as f32).ceil() as u8;
    if after < before && after > 0 {
        events.0.push(super::SimulationEvent::Countdown(after));
    }
    if session.countdown_ticks_remaining == 0 {
        session.phase = MatchPhase::Running;
        events.0.push(super::SimulationEvent::Go);
    }
}
