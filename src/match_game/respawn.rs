use bevy::{prelude::*, time::Fixed};

use crate::{
    board::{BoardGrid, DeterministicRng},
    config::GameConfig,
    ids::{CompetitorId, MAX_COMPETITORS},
    movement::CompetitorMotion,
    territory_map::TerritoryMap,
    trail::ActiveTrail,
};

use super::model::{
    Competitor, DisplacementCredits, LastOwnedCell, LifeState, LifeStatus, MatchPhase,
    MatchSession, MatchStatistics, SimulationEvent, SimulationEvents, SpawnProtection,
    TerritoryRecord,
};

type RespawnSnapshot = (
    &'static Competitor,
    &'static CompetitorMotion,
    &'static LifeState,
    Option<&'static ActiveTrail>,
);
type RespawnControl = (
    Entity,
    &'static Competitor,
    &'static mut CompetitorMotion,
    &'static mut LifeState,
    &'static mut SpawnProtection,
    &'static mut TerritoryRecord,
    &'static mut MatchStatistics,
    &'static mut LastOwnedCell,
    Option<&'static ActiveTrail>,
);

#[allow(clippy::too_many_arguments)]
pub(super) fn advance_respawns(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    config: Res<GameConfig>,
    mut board: ResMut<BoardGrid>,
    mut territory_map: ResMut<TerritoryMap>,
    session: Res<MatchSession>,
    mut events: ResMut<SimulationEvents>,
    mut displacement_credits: ResMut<DisplacementCredits>,
    mut queries: ParamSet<(Query<RespawnSnapshot>, Query<RespawnControl>)>,
    mut living_scratch: Local<Vec<(CompetitorId, Vec2)>>,
    mut reserved_scratch: Local<Vec<Vec2>>,
) {
    if session.phase != MatchPhase::Running {
        return;
    }
    living_scratch.clear();
    living_scratch.extend(
        queries
            .p0()
            .iter()
            .filter(|(_, _, life, _)| life.is_alive())
            .map(|(competitor, motion, _, _)| (competitor.id, motion.position)),
    );
    living_scratch.sort_by(|(a, _), (b, _)| {
        territory_map
            .area(*b)
            .total_cmp(&territory_map.area(*a))
            .then_with(|| a.cmp(b))
    });
    reserved_scratch.clear();
    for (
        entity,
        competitor,
        mut motion,
        mut life,
        mut protection,
        mut territory,
        mut stats,
        mut last_owned,
        trail,
    ) in queries.p1().iter_mut()
    {
        if life.is_alive() {
            continue;
        }
        life.respawn_remaining = (life.respawn_remaining - time.delta_secs()).max(0.0);
        if life.respawn_remaining > 0.0 {
            continue;
        }
        let position = choose_respawn(
            &board,
            competitor.id,
            session.seed ^ stats.deaths as u64,
            &living_scratch,
            &reserved_scratch,
            &config,
        );
        reserved_scratch.push(position);
        if trail.is_some() {
            commands.entity(entity).remove::<ActiveTrail>();
        }
        claim_respawn_seed(
            &mut board,
            &mut territory_map,
            position,
            config.starting_territory_radius,
            competitor.id,
            &living_scratch,
            &mut displacement_credits,
        );
        reset_respawn_anchor(&board, &mut motion, &mut last_owned, position);
        living_scratch.push((competitor.id, position));
        life.status = LifeStatus::Alive;
        protection.remaining = config.spawn_protection_seconds;
        protection.elapsed = 0.0;
        territory.current_area = territory_map.area(competitor.id);
        territory.peak_area = territory.peak_area.max(territory.current_area);
        territory.current_cells = board.owner_counts[competitor.id.index()];
        territory.peak_cells = territory.peak_cells.max(territory.current_cells);
        stats.peak_territory_area = stats.peak_territory_area.max(territory.current_area);
        stats.peak_territory_cells = stats.peak_territory_cells.max(territory.current_cells);
        events.0.push(SimulationEvent::Respawn {
            player: competitor.id,
        });
    }
}

fn choose_respawn(
    board: &BoardGrid,
    id: CompetitorId,
    seed: u64,
    living: &[(CompetitorId, Vec2)],
    reserved: &[Vec2],
    config: &GameConfig,
) -> Vec2 {
    let mut rng = DeterministicRng::new(seed ^ (id.0 as u64 + 1).wrapping_mul(0x9e37_79b9));
    let leader = living.first().map(|player| player.1).unwrap_or(Vec2::ZERO);
    let candidates = &board.spawn_candidates;
    let mut best = None;
    for relax in [1.0, 0.65, 0.0] {
        for _ in 0..256.min(candidates.len()) {
            let index = candidates[rng.index(candidates.len())];
            let position = board.cell_center(board.cell(index));
            let cube_distance = living
                .iter()
                .map(|(_, other)| other.distance(position))
                .fold(f32::INFINITY, f32::min);
            let trail_distance =
                nearest_active_trail_distance(board, position, config.spawn_trail_clearance);
            if cube_distance < config.spawn_cube_clearance * relax
                || trail_distance < config.spawn_trail_clearance * relax
                || reserved.iter().any(|other| {
                    other.distance(position)
                        < config
                            .spawn_cube_clearance
                            .max(config.starting_territory_radius * 2.0)
                })
            {
                continue;
            }
            let unclaimed =
                spawn_disk_unclaimed_ratio(board, position, config.starting_territory_radius)
                    * 100.0;
            let score = unclaimed
                + cube_distance.min(30.0)
                + trail_distance.min(20.0)
                + position.distance(leader) * 0.5
                + board.signed_distance[index];
            if best.is_none_or(|(_, current_score)| score > current_score) {
                best = Some((position, score));
            }
        }
        if best.is_some() {
            break;
        }
    }
    best.map(|candidate| candidate.0).unwrap_or_else(|| {
        candidates
            .iter()
            .map(|&index| board.cell_center(board.cell(index)))
            .max_by(|a, b| {
                let safety = |point: Vec2| {
                    living
                        .iter()
                        .map(|(_, other)| point.distance_squared(*other))
                        .chain(reserved.iter().map(|other| point.distance_squared(*other)))
                        .fold(f32::INFINITY, f32::min)
                };
                safety(*a).total_cmp(&safety(*b))
            })
            .unwrap_or_else(|| board.nearest_interior(Vec2::ZERO, config.spawn_boundary_clearance))
    })
}

fn nearest_active_trail_distance(board: &BoardGrid, point: Vec2, limit: f32) -> f32 {
    let Some((min, max)) =
        board.clamped_cell_bounds(point - Vec2::splat(limit), point + Vec2::splat(limit))
    else {
        return f32::INFINITY;
    };
    let cell_radius = board.cell_size * std::f32::consts::FRAC_1_SQRT_2;
    let mut nearest = f32::INFINITY;
    for y in min.y..=max.y {
        for x in min.x..=max.x {
            let index = board.index(crate::board::Cell::new(x, y)).unwrap();
            if board.active_trail_bits[index] != 0 {
                nearest = nearest.min(
                    (board.cell_center(board.cell(index)).distance(point) - cell_radius).max(0.0),
                );
            }
        }
    }
    nearest
}

fn spawn_disk_unclaimed_ratio(board: &BoardGrid, center: Vec2, radius: f32) -> f32 {
    let radius_squared = radius * radius;
    let mut playable = 0_u32;
    let mut unclaimed = 0_u32;
    let Some((min, max)) =
        board.clamped_cell_bounds(center - Vec2::splat(radius), center + Vec2::splat(radius))
    else {
        return 0.0;
    };
    for y in min.y..=max.y {
        for x in min.x..=max.x {
            let cell = crate::board::Cell::new(x, y);
            let Some(index) = board.index(cell) else {
                continue;
            };
            if board.field_mask[index]
                && board.cell_center(cell).distance_squared(center) <= radius_squared
            {
                playable += 1;
                unclaimed += u32::from(board.owner[index].competitor().is_none());
            }
        }
    }
    if playable == 0 {
        0.0
    } else {
        unclaimed as f32 / playable as f32
    }
}

pub(super) fn reset_respawn_anchor(
    board: &BoardGrid,
    motion: &mut CompetitorMotion,
    last_owned: &mut LastOwnedCell,
    position: Vec2,
) {
    motion.position = position;
    motion.previous_position = position;
    motion.heading = (-position).try_normalize().unwrap_or(Vec2::Y);
    last_owned.0 = board
        .world_to_cell(position)
        .expect("respawn candidates are playable board cells");
}

pub(super) fn claim_respawn_seed(
    board: &mut BoardGrid,
    territory_map: &mut TerritoryMap,
    position: Vec2,
    radius: f32,
    respawning: CompetitorId,
    occupants: &[(CompetitorId, Vec2)],
    credits: &mut DisplacementCredits,
) {
    let previous_areas: [f32; MAX_COMPETITORS] =
        std::array::from_fn(|index| territory_map.area(CompetitorId(index as u8)));
    let revision = territory_map.revision;
    let result = territory_map.seed_owner(position, radius, respawning);
    if territory_map.revision != revision {
        territory_map.refresh_sample_cache(board, &result.claim);
        for (_, removed) in &result.disconnected_by_owner {
            territory_map.refresh_sample_cache(board, removed);
        }
    }
    for (index, previous) in previous_areas.into_iter().enumerate() {
        let victim = CompetitorId(index as u8);
        if victim != respawning && previous > 1e-4 && territory_map.area(victim) <= 1e-4 {
            credits.0.push((victim, respawning));
        }
    }
    credits.0.extend(
        occupants
            .iter()
            .filter(|(owner, position)| {
                result
                    .disconnected_by_owner
                    .iter()
                    .any(|(removed_owner, removed)| {
                        owner == removed_owner && removed.contains_world(*position)
                    })
            })
            .map(|(owner, _)| (*owner, respawning)),
    );
    credits.0.sort_unstable();
    credits.0.dedup();
}
