use bevy::{prelude::*, time::Fixed};

use crate::{
    app_state::AppState,
    board::{BoardGrid, choose_initial_spawns},
    config::GameConfig,
    ids::CompetitorId,
    input::{ControlSource, HumanController, SteeringIntent},
    lobby::MatchSetup,
    movement::CompetitorMotion,
    npc::{NpcController, NpcDifficulty, deterministic_npc_name, deterministic_personality},
};

use super::model::*;

pub(super) fn begin_from_lobby(world: &mut World) {
    let setup = world.resource::<MatchSetup>().clone();
    start_match(world, &setup);
    world.resource_mut::<MatchLoadingFrames>().0 = 0;
}

#[derive(Resource, Default)]
pub(super) struct MatchLoadingFrames(pub u8);

const MATCH_LOADING_MIN_FRAMES: u8 = 6;

pub(super) fn finish_match_loading(
    mut frames: ResMut<MatchLoadingFrames>,
    mut next: ResMut<NextState<AppState>>,
) {
    frames.0 = frames.0.saturating_add(1);
    if frames.0 >= MATCH_LOADING_MIN_FRAMES {
        next.set(AppState::Countdown);
    }
}

/// Fully resets authoritative state using a lobby composition. Useful for rematches and tests.
pub fn start_match(world: &mut World, setup: &MatchSetup) {
    despawn_competitors(world);
    let config = world.resource::<GameConfig>().clone();
    let count = usize::from(setup.total_competitors.clamp(2, 12));
    let mut board = BoardGrid::generate(setup.seed, count, &config);
    let spawns = choose_initial_spawns(&board, count, 8.0, setup.seed);
    for (index, &spawn) in spawns.iter().enumerate() {
        board.claim_disk(
            spawn,
            config.starting_territory_radius,
            CompetitorId(index as u8),
        );
    }
    let counts = board.owner_counts;
    world.insert_resource(board);
    world.insert_resource(MatchSession {
        seed: setup.seed,
        phase: MatchPhase::Countdown,
        elapsed_seconds: 0.0,
        countdown_remaining: 3.0,
        winner: None,
        result_hold_remaining: 0.0,
    });
    world.insert_resource(Rankings::default());
    world.resource_mut::<SimulationEvents>().0.clear();
    world.insert_resource(EliminationFeed::default());
    for (index, &position) in spawns.iter().enumerate() {
        spawn_competitor(world, setup, &config, counts[index], index, position);
    }
}

fn spawn_competitor(
    world: &mut World,
    setup: &MatchSetup,
    config: &GameConfig,
    territory_cells: u32,
    index: usize,
    position: Vec2,
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
            current_cells: territory_cells,
            peak_cells: territory_cells,
        },
        MatchStatistics {
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
        let personality = deterministic_personality(setup.seed, id);
        world.spawn((
            base,
            Competitor {
                id,
                display_name: deterministic_npc_name(index).to_owned(),
                kind: CompetitorKind::Npc,
                color_id: index as u8,
                pattern_id: (index % 8) as u8,
            },
            NpcController::standard(id, personality, NpcDifficulty::Normal),
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

pub(super) fn cleanup_match(
    mut commands: Commands,
    competitors: Query<Entity, With<Competitor>>,
    mut board: ResMut<BoardGrid>,
    mut session: ResMut<MatchSession>,
    mut rankings: ResMut<Rankings>,
    mut events: ResMut<SimulationEvents>,
    feed: Option<ResMut<EliminationFeed>>,
) {
    for entity in &competitors {
        commands.entity(entity).despawn();
    }
    *board = BoardGrid::default();
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
