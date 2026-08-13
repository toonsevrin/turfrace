use bevy::{prelude::*, time::Fixed};

use crate::{
    app_state::AppState,
    board::{BoardGrid, choose_initial_spawns},
    config::GameConfig,
    ids::CompetitorId,
    input::{ControlSource, HumanController, SteeringIntent},
    lobby::{MatchLaunchMode, MatchSetup},
    movement::CompetitorMotion,
    npc::{
        NpcController, NpcDifficulty, NpcEvent, NpcEventQueue, NpcRosterEntry, generate_npc_roster,
    },
    territory_map::TerritoryMap,
};

use super::model::*;

pub(super) fn begin_from_lobby(world: &mut World) {
    let setup = world.resource::<MatchSetup>().clone();
    let mode = std::mem::take(&mut *world.resource_mut::<MatchLaunchMode>());
    start_match(world, &setup);
    if mode == MatchLaunchMode::LobbyCountdownCompleted {
        let mut session = world.resource_mut::<MatchSession>();
        session.phase = MatchPhase::Running;
        session.countdown_remaining = 0.0;
    }
    *world.resource_mut::<MatchLoadingFrames>() = MatchLoadingFrames {
        elapsed: 0,
        destination: match mode {
            MatchLaunchMode::StandardCountdown => AppState::Countdown,
            MatchLaunchMode::LobbyCountdownCompleted => AppState::Playing,
        },
    };
}

const ATTRACT_NPC_COUNT: u8 = 6;
const ATTRACT_SEED_START: u64 = 0xA77A_C7A5_5EED;
const SEED_STEP: u64 = 6_364_136_223_846_793_005;

#[derive(Resource, Debug, Clone, Copy)]
pub(super) struct AttractSeedSequence(pub u64);

impl Default for AttractSeedSequence {
    fn default() -> Self {
        Self(ATTRACT_SEED_START)
    }
}

pub(super) fn begin_attract_match(world: &mut World) {
    let seed = {
        let mut sequence = world.resource_mut::<AttractSeedSequence>();
        let seed = sequence.0;
        sequence.0 = seed.wrapping_mul(SEED_STEP).wrapping_add(1);
        seed
    };
    let setup = MatchSetup {
        field_seed: seed,
        npc_roster_seed: seed ^ 0x4e50_4352_4f53_5445,
        npc_difficulty: NpcDifficulty::Normal,
        total_competitors: ATTRACT_NPC_COUNT,
        humans: Vec::new(),
        replay_same_field: false,
    };
    start_match_for(world, &setup, MatchPurpose::Attract);
}

#[derive(Resource)]
pub(super) struct MatchLoadingFrames {
    elapsed: u8,
    pub(super) destination: AppState,
}

impl Default for MatchLoadingFrames {
    fn default() -> Self {
        Self {
            elapsed: 0,
            destination: AppState::Countdown,
        }
    }
}

const MATCH_LOADING_MIN_FRAMES: u8 = 6;

pub(super) fn finish_match_loading(
    mut frames: ResMut<MatchLoadingFrames>,
    mut next: ResMut<NextState<AppState>>,
) {
    frames.elapsed = frames.elapsed.saturating_add(1);
    if frames.elapsed >= MATCH_LOADING_MIN_FRAMES {
        next.set(frames.destination);
    }
}

/// Fully resets authoritative state using a lobby composition. Useful for rematches and tests.
pub fn start_match(world: &mut World, setup: &MatchSetup) {
    start_match_for(world, setup, MatchPurpose::Playable);
}

pub(super) fn start_match_for(world: &mut World, setup: &MatchSetup, purpose: MatchPurpose) {
    if !world.contains_resource::<NpcEventQueue>() {
        world.insert_resource(NpcEventQueue::default());
    }
    despawn_competitors(world);
    let config = world.resource::<GameConfig>().clone();
    let count = usize::from(setup.total_competitors.clamp(2, 12));
    let mut board = BoardGrid::generate(setup.field_seed, count, &config);
    let mut territory = TerritoryMap::from_board(&board);
    let spawns = choose_initial_spawns(&board, count, 8.0, setup.field_seed);
    for (index, &spawn) in spawns.iter().enumerate() {
        territory.seed_owner(
            spawn,
            config.starting_territory_radius,
            CompetitorId(index as u8),
        );
    }
    // The grid is now a derived sample cache used by broadphase and legacy
    // integrations. Gameplay ownership remains in the vector map.
    territory.rebuild_sample_cache(&mut board);
    let counts = board.owner_counts;
    let (phase, countdown_remaining) = match purpose {
        MatchPurpose::Playable => (MatchPhase::Countdown, 3.0),
        MatchPurpose::Attract => (MatchPhase::Running, 0.0),
    };
    world.insert_resource(board);
    world.insert_resource(territory);
    world.insert_resource(MatchSession {
        field_seed: setup.field_seed,
        npc_roster_seed: setup.npc_roster_seed,
        purpose,
        phase,
        elapsed_seconds: 0.0,
        countdown_remaining,
        winner: None,
        result_hold_remaining: 0.0,
    });
    world.insert_resource(Rankings::default());
    world.resource_mut::<SimulationEvents>().0.clear();
    world.resource_mut::<NpcEventQueue>().0.clear();
    world.insert_resource(EliminationFeed::default());
    let npc_count = count.saturating_sub(setup.humans.len());
    let npc_roster = generate_npc_roster(setup.npc_roster_seed, npc_count, setup.npc_difficulty);
    for (index, &position) in spawns.iter().enumerate() {
        spawn_competitor(
            world,
            setup,
            &config,
            counts[index],
            world
                .resource::<TerritoryMap>()
                .area(CompetitorId(index as u8)),
            index,
            position,
            index
                .checked_sub(setup.humans.len())
                .and_then(|npc_index| npc_roster.get(npc_index)),
        );
        if index >= setup.humans.len() {
            world
                .resource_mut::<NpcEventQueue>()
                .0
                .push((CompetitorId(index as u8), NpcEvent::Spawned));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_competitor(
    world: &mut World,
    setup: &MatchSetup,
    config: &GameConfig,
    territory_cells: u32,
    territory_area: f32,
    index: usize,
    position: Vec2,
    npc_entry: Option<&NpcRosterEntry>,
) {
    let id = CompetitorId(index as u8);
    let heading = (-position).try_normalize().unwrap_or(Vec2::Y);
    let base = (
        id,
        CompetitorMotion::new(position, heading),
        LifeState::alive(),
        SpawnProtection {
            remaining: config.spawn_protection_seconds,
            elapsed: 0.0,
        },
        TerritoryRecord {
            current_area: territory_area,
            peak_area: territory_area,
            current_cells: territory_cells,
            peak_cells: territory_cells,
        },
        MatchStatistics {
            peak_territory_area: territory_area,
            peak_territory_cells: territory_cells,
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
            source: ControlSource::Npc,
        },
    );
    if let Some(human) = setup.humans.get(index) {
        world.spawn((
            base,
            Competitor {
                id,
                display_name: human.display_name.clone(),
                kind: CompetitorKind::Human,
                color_id: human.color_id,
                pattern_id: human.pattern_id,
            },
            HumanController {
                device: human.device,
            },
        ));
    } else {
        let npc_entry = npc_entry.expect("every NPC slot has a deterministic roster entry");
        world.spawn((
            base,
            Competitor {
                id,
                display_name: npc_entry.name.clone(),
                kind: CompetitorKind::Npc,
                color_id: index as u8,
                pattern_id: (index % 8) as u8,
            },
            NpcController::from_roster(id, npc_entry, index),
        ));
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

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(super) fn cleanup_match(
    mut commands: Commands,
    competitors: Query<Entity, With<Competitor>>,
    mut board: ResMut<BoardGrid>,
    mut territory: ResMut<TerritoryMap>,
    mut session: ResMut<MatchSession>,
    mut rankings: ResMut<Rankings>,
    mut events: ResMut<SimulationEvents>,
    feed: Option<ResMut<EliminationFeed>>,
) {
    for entity in &competitors {
        commands.entity(entity).despawn();
    }
    *board = BoardGrid::default();
    *territory = TerritoryMap::default();
    *session = MatchSession::default();
    rankings.entries.clear();
    events.0.clear();
    if let Some(mut feed) = feed {
        feed.0.clear();
    }
}

pub(super) fn advance_countdown(
    time: Res<Time<Fixed>>,
    mut session: ResMut<MatchSession>,
    mut events: ResMut<SimulationEvents>,
    mut next: ResMut<NextState<AppState>>,
) {
    let before = session.countdown_remaining.ceil() as u8;
    session.countdown_remaining = (session.countdown_remaining - time.delta_secs()).max(0.0);
    let after = session.countdown_remaining.ceil() as u8;
    if after < before && after > 0 {
        events.0.push(SimulationEvent::Countdown(after));
    }
    if session.countdown_remaining <= 0.0 {
        session.phase = MatchPhase::Running;
        events.0.push(SimulationEvent::Go);
        next.set(AppState::Playing);
    }
}

pub(super) fn advance_result_hold(
    time: Res<Time>,
    mut session: ResMut<MatchSession>,
    mut next: ResMut<NextState<AppState>>,
) {
    session.result_hold_remaining = (session.result_hold_remaining - time.delta_secs()).max(0.0);
    if session.result_hold_remaining <= 0.0 {
        next.set(AppState::Results);
    }
}
