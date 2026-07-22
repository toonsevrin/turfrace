use bevy::{ecs::system::SystemParam, prelude::*, time::Fixed};

use crate::{
    app_state::AppState,
    board::{BoardGrid, DeterministicRng},
    capture::{apply_equal_time_captures, calculate_capture},
    combat::{CollisionBody, collect_collision_intents},
    config::GameConfig,
    ids::CompetitorId,
    input::{ControlSource, SteeringIntent},
    movement::{CompetitorMotion, advance_motion, segment_cell_entry_time},
    npc::{
        BoardQuery, NpcContext, NpcController, NpcSelfState, PerceivedCompetitor, PerceivedTrail,
        RankingSnapshot,
    },
    trail::{ActiveTrail, clear_trail_bits, update_trail_raster},
};

use super::lifecycle::{advance_countdown, advance_result_hold, begin_from_lobby, cleanup_match};
use super::model::*;

type NpcSnapshot = (
    &'static Competitor,
    &'static CompetitorMotion,
    &'static LifeState,
    &'static TerritoryRecord,
    Option<&'static ActiveTrail>,
);
type NpcControl = (
    &'static Competitor,
    &'static CompetitorMotion,
    &'static LifeState,
    &'static SpawnProtection,
    Option<&'static ActiveTrail>,
    &'static mut NpcController,
    &'static mut SteeringIntent,
);
type TrailExtension = (
    Entity,
    &'static Competitor,
    &'static CompetitorMotion,
    &'static LifeState,
    &'static SpawnProtection,
    &'static mut LastOwnedCell,
    Option<&'static mut ActiveTrail>,
);
type TerritoryConsequences = (
    Entity,
    &'static Competitor,
    &'static CompetitorMotion,
    &'static mut LifeState,
    &'static SpawnProtection,
    &'static mut TerritoryRecord,
    &'static mut MatchStatistics,
    &'static LastOwnedCell,
    Option<&'static ActiveTrail>,
);
type RespawnSnapshot = (
    &'static Competitor,
    &'static CompetitorMotion,
    &'static LifeState,
    Option<&'static ActiveTrail>,
);
type RespawnControl = (
    &'static Competitor,
    &'static mut CompetitorMotion,
    &'static mut LifeState,
    &'static mut SpawnProtection,
    &'static mut TerritoryRecord,
    &'static mut MatchStatistics,
);

#[derive(SystemParam)]
struct EliminationResources<'w> {
    events: ResMut<'w, SimulationEvents>,
    session: Option<Res<'w, MatchSession>>,
    feed: Option<ResMut<'w, EliminationFeed>>,
}

pub struct MatchPlugin;

impl Plugin for MatchPlugin {
    fn build(&self, app: &mut App) {
        let sets = (
            MatchSystemSet::PollInput,
            MatchSystemSet::NpcThink,
            MatchSystemSet::BuildSteeringIntent,
            MatchSystemSet::MoveCompetitors,
            MatchSystemSet::ExtendTrails,
            MatchSystemSet::DetectTrailCollisions,
            MatchSystemSet::ResolveDeaths,
            MatchSystemSet::DetectClosures,
            MatchSystemSet::ResolveCaptures,
            MatchSystemSet::ResolveTerritoryConsequences,
            MatchSystemSet::CheckVictory,
            MatchSystemSet::AdvanceRespawns,
            MatchSystemSet::UpdateRankings,
        )
            .chain();
        app.init_resource::<GameConfig>()
            .init_resource::<BoardGrid>()
            .init_resource::<MatchSession>()
            .init_resource::<Rankings>()
            .init_resource::<SimulationEvents>()
            .init_resource::<EliminationFeed>()
            .init_resource::<PendingDeaths>()
            .init_resource::<PendingCaptures>()
            .init_resource::<DisplacementCredits>()
            .init_resource::<Time<Fixed>>()
            .configure_sets(FixedUpdate, sets)
            .add_systems(Startup, configure_fixed_timestep)
            .add_systems(OnEnter(AppState::MatchLoading), begin_from_lobby)
            .add_systems(OnEnter(AppState::Lobby), cleanup_match)
            .add_systems(OnEnter(AppState::Home), cleanup_match)
            .add_systems(
                FixedUpdate,
                advance_countdown.run_if(in_state(AppState::Countdown)),
            )
            .add_systems(
                FixedUpdate,
                npc_think
                    .in_set(MatchSystemSet::NpcThink)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                move_competitors
                    .in_set(MatchSystemSet::MoveCompetitors)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                extend_trails
                    .in_set(MatchSystemSet::ExtendTrails)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                detect_trail_collisions
                    .in_set(MatchSystemSet::DetectTrailCollisions)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                resolve_deaths
                    .in_set(MatchSystemSet::ResolveDeaths)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                detect_closures
                    .in_set(MatchSystemSet::DetectClosures)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                resolve_captures
                    .in_set(MatchSystemSet::ResolveCaptures)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                resolve_territory_consequences
                    .in_set(MatchSystemSet::ResolveTerritoryConsequences)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                check_victory
                    .in_set(MatchSystemSet::CheckVictory)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                advance_respawns
                    .in_set(MatchSystemSet::AdvanceRespawns)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                update_rankings
                    .in_set(MatchSystemSet::UpdateRankings)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                Update,
                advance_result_hold.run_if(in_state(AppState::GameOver)),
            );
    }
}

fn configure_fixed_timestep(config: Res<GameConfig>, mut fixed_time: ResMut<Time<Fixed>>) {
    fixed_time.set_timestep_hz(config.fixed_hz);
}

fn npc_think(
    time: Res<Time<Fixed>>,
    board: Res<BoardGrid>,
    session: Res<MatchSession>,
    rankings: Res<Rankings>,
    mut queries: ParamSet<(Query<NpcSnapshot>, Query<NpcControl>)>,
) {
    let people: Vec<PerceivedCompetitor> = queries
        .p0()
        .iter()
        .map(|(c, m, l, t, _)| PerceivedCompetitor {
            id: c.id,
            position: m.position,
            alive: l.is_alive(),
            territory_cells: t.current_cells,
        })
        .collect();
    let trail_snapshots: Vec<(CompetitorId, Vec<Vec2>)> = queries
        .p0()
        .iter()
        .filter_map(|(c, _, _, _, t)| t.map(|t| (c.id, t.points.clone())))
        .collect();
    let ranking = RankingSnapshot {
        ordered: rankings.entries.iter().map(|e| e.id).collect(),
    };
    for (competitor, motion, life, protection, trail, mut controller, mut steering) in
        queries.p1().iter_mut()
    {
        if !life.is_alive() {
            continue;
        }
        controller.think_remaining -= time.delta_secs();
        if controller.think_remaining > 0.0 {
            continue;
        }
        controller.think_remaining += 1.0 / controller.difficulty.think_hz();
        let radius = controller.difficulty.perception();
        let nearby_people: Vec<_> = people
            .iter()
            .copied()
            .filter(|p| p.id != competitor.id && p.position.distance(motion.position) <= radius)
            .collect();
        let nearby_trails: Vec<_> = trail_snapshots
            .iter()
            .filter(|(owner, _)| *owner != competitor.id)
            .filter_map(|(owner, points)| {
                points
                    .iter()
                    .copied()
                    .min_by(|a, b| {
                        a.distance_squared(motion.position)
                            .total_cmp(&b.distance_squared(motion.position))
                    })
                    .map(|p| PerceivedTrail {
                        owner: *owner,
                        nearest_point: p,
                        distance: p.distance(motion.position),
                    })
            })
            .filter(|t| t.distance <= radius)
            .collect();
        let query = BoardQuery::new(&board, competitor.id);
        let context = NpcContext {
            self_state: NpcSelfState {
                id: competitor.id,
                position: motion.position,
                heading: motion.heading,
                protected: protection.active(),
                trail_length: trail.map_or(0.0, |t| t.length),
                owns_current_cell: query.owns(motion.position),
            },
            nearby_competitors: &nearby_people,
            nearby_trails: &nearby_trails,
            board: &query,
            ranking: &ranking,
            match_time: session.elapsed_seconds,
        };
        let think_hz = controller.difficulty.think_hz();
        let intent = controller.brain.tick(&context, 1.0 / think_hz);
        controller.last_intent = intent;
        let error = controller.difficulty.error_radians()
            * (session.elapsed_seconds * 1.7 + competitor.id.0 as f32).sin();
        let (sin, cos) = error.sin_cos();
        let d = intent.desired_direction;
        steering.desired_direction =
            Vec2::new(d.x * cos - d.y * sin, d.x * sin + d.y * cos).normalize_or_zero();
        steering.magnitude = 1.0;
        steering.source = ControlSource::Npc;
    }
}

fn move_competitors(
    time: Res<Time<Fixed>>,
    config: Res<GameConfig>,
    board: Res<BoardGrid>,
    mut session: ResMut<MatchSession>,
    mut query: Query<(
        &Competitor,
        &mut CompetitorMotion,
        &SteeringIntent,
        &LifeState,
        &mut SpawnProtection,
    )>,
) {
    let dt = time.delta_secs();
    session.elapsed_seconds += dt;
    for (competitor, mut motion, intent, life, mut protection) in &mut query {
        if !life.is_alive() {
            continue;
        }
        let desired = (intent.magnitude > 0.0).then_some(intent.desired_direction);
        advance_motion(&mut motion, desired, &board, &config, dt);
        advance_spawn_protection(
            &mut protection,
            competitor.id,
            motion.position,
            &board,
            &config,
            dt,
        );
    }
}

fn advance_spawn_protection(
    protection: &mut SpawnProtection,
    competitor: CompetitorId,
    position: Vec2,
    board: &BoardGrid,
    config: &GameConfig,
    delta_seconds: f32,
) {
    if !protection.active() {
        return;
    }
    protection.elapsed += delta_seconds;
    protection.remaining = (protection.remaining - delta_seconds).max(0.0);
    if protection.elapsed >= config.spawn_protection_minimum_seconds
        && board
            .world_to_cell(position)
            .is_some_and(|cell| !board.owns(cell, competitor))
    {
        protection.remaining = 0.0;
    }
}

fn boundary_crossing(board: &BoardGrid, player: CompetitorId, from: Vec2, to: Vec2) -> Vec2 {
    let mut lo = 0.0;
    let mut hi = 1.0;
    for _ in 0..14 {
        let mid = (lo + hi) * 0.5;
        let owned = board
            .world_to_cell(from.lerp(to, mid))
            .is_some_and(|c| board.owns(c, player));
        if owned { lo = mid } else { hi = mid }
    }
    from.lerp(to, hi)
}

fn extend_trails(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut board: ResMut<BoardGrid>,
    mut events: ResMut<SimulationEvents>,
    mut query: Query<TrailExtension>,
) {
    for (entity, competitor, motion, life, protection, mut last_owned, trail) in &mut query {
        if !life.is_alive() || protection.active() {
            continue;
        }
        let Some(current) = board.world_to_cell(motion.position) else {
            continue;
        };
        let currently_owned = board.owns(current, competitor.id);
        if let Some(mut trail) = trail {
            trail.append_exact(motion.position);
            trail.last_sample_heading = motion.heading;
            update_trail_raster(&mut board, &mut trail, config.trail_width);
        } else if currently_owned {
            last_owned.0 = current;
        } else {
            let from = if board
                .world_to_cell(motion.previous_position)
                .is_some_and(|c| board.owns(c, competitor.id))
            {
                motion.previous_position
            } else {
                board.cell_center(last_owned.0)
            };
            let boundary = boundary_crossing(&board, competitor.id, from, motion.position);
            let mut trail = ActiveTrail::new(competitor.id, last_owned.0, boundary, motion.heading);
            trail.append_exact(motion.position);
            update_trail_raster(&mut board, &mut trail, config.trail_width);
            commands.entity(entity).insert(trail);
            events.0.push(SimulationEvent::TrailStarted {
                player: competitor.id,
            });
        }
    }
}

fn detect_trail_collisions(
    config: Res<GameConfig>,
    bodies: Query<(&Competitor, &CompetitorMotion, &LifeState, &SpawnProtection)>,
    trails: Query<&ActiveTrail>,
    mut pending: ResMut<PendingDeaths>,
) {
    let snapshots: Vec<_> = bodies
        .iter()
        .filter(|(_, _, l, _)| l.is_alive())
        .map(|(c, m, _, p)| CollisionBody {
            id: c.id,
            previous: m.previous_position,
            current: m.position,
            protected: p.active(),
        })
        .collect();
    let trails: Vec<&ActiveTrail> = trails.iter().collect();
    pending.0 = collect_collision_intents(
        &snapshots,
        &trails,
        config.collision_radius,
        config.trail_width,
        config.self_trail_exclusion_distance,
    );
}

fn resolve_deaths(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut board: ResMut<BoardGrid>,
    mut pending: ResMut<PendingDeaths>,
    mut eliminations: EliminationResources,
    mut query: Query<(
        Entity,
        &Competitor,
        &mut LifeState,
        &mut MatchStatistics,
        Option<&ActiveTrail>,
    )>,
) {
    let intents = std::mem::take(&mut pending.0);
    for intent in &intents {
        if let Some(killer) = intent.killer
            && let Some((_, _, _, mut stats, _)) =
                query.iter_mut().find(|(_, c, _, _, _)| c.id == killer)
        {
            stats.kills += 1;
        }
    }
    for intent in intents {
        if let Some((entity, competitor, mut life, mut stats, trail)) = query
            .iter_mut()
            .find(|(_, c, _, _, _)| c.id == intent.victim)
        {
            if !life.is_alive() {
                continue;
            }
            if let Some(trail) = trail {
                clear_trail_bits(&mut board, competitor.id, &trail.cells);
                commands.entity(entity).remove::<ActiveTrail>();
            }
            board.clear_owner(competitor.id);
            stats.deaths += 1;
            life.status = LifeStatus::Respawning;
            life.respawn_remaining = config.respawn_delay(stats.deaths);
            eliminations.events.0.push(SimulationEvent::Death {
                victim: competitor.id,
                killer: intent.killer,
                cause: if intent.killer.is_some() {
                    DeathCause::TrailCut
                } else {
                    DeathCause::SelfTrail
                },
            });
            let match_time = eliminations
                .session
                .as_ref()
                .map_or(0.0, |session| session.elapsed_seconds);
            if let Some(feed) = eliminations.feed.as_deref_mut() {
                feed.push(EliminationRecord {
                    victim: competitor.id,
                    killer: intent.killer,
                    cause: if intent.killer.is_some() {
                        DeathCause::TrailCut
                    } else {
                        DeathCause::SelfTrail
                    },
                    match_time,
                });
            }
        }
    }
}

fn detect_closures(
    board: Res<BoardGrid>,
    mut pending: ResMut<PendingCaptures>,
    query: Query<(
        Entity,
        &Competitor,
        &CompetitorMotion,
        &LifeState,
        &ActiveTrail,
    )>,
) {
    pending.0.clear();
    for (entity, c, m, l, trail) in &query {
        if !l.is_alive() {
            continue;
        }
        let Some(end) = board.world_to_cell(m.position) else {
            continue;
        };
        if board.owns(end, c.id) {
            let time = segment_cell_entry_time(&board, m.previous_position, m.position, |cell| {
                board.owns(cell, c.id)
            })
            .unwrap_or(1.0);
            pending.0.push(PendingCapture {
                player: c.id,
                entity,
                time,
                trail: trail.clone(),
                end,
            });
        }
    }
}

fn resolve_captures(
    mut commands: Commands,
    mut board: ResMut<BoardGrid>,
    mut pending: ResMut<PendingCaptures>,
    mut displaced: ResMut<DisplacementCredits>,
    mut events: ResMut<SimulationEvents>,
    mut query: Query<(&Competitor, &mut MatchStatistics)>,
) {
    pending.0.sort_by(|a, b| {
        a.time
            .total_cmp(&b.time)
            .then_with(|| a.player.cmp(&b.player))
    });
    let items = std::mem::take(&mut pending.0);
    let mut offset = 0;
    while offset < items.len() {
        let mut end = offset + 1;
        while end < items.len() && (items[end].time - items[offset].time).abs() <= 1e-5 {
            end += 1
        }
        let group = &items[offset..end];
        let mut captures: Vec<_> = group
            .iter()
            .map(|p| {
                (
                    p.player,
                    calculate_capture(&board, p.player, &p.trail, p.end),
                )
            })
            .collect();
        apply_equal_time_captures(&mut board, &mut captures);
        for pending in group {
            clear_trail_bits(&mut board, pending.player, &pending.trail.cells);
            commands.entity(pending.entity).remove::<ActiveTrail>();
            let result = captures
                .iter()
                .find(|(id, _)| *id == pending.player)
                .unwrap()
                .1
                .clone();
            let stolen: u32 = result.stolen_by_owner.iter().map(|(_, n)| *n).sum();
            for (victim, _) in &result.stolen_by_owner {
                if board.owner_counts[victim.index()] == 0 {
                    displaced.0.push((*victim, pending.player));
                }
            }
            if let Some((_, mut stats)) = query.iter_mut().find(|(c, _)| c.id == pending.player) {
                let cells = result.claimed_cells.len() as u32;
                stats.captures_completed += 1;
                stats.cells_captured_total += cells;
                stats.cells_stolen_total += stolen;
                stats.largest_capture_cells = stats.largest_capture_cells.max(cells);
            }
            events.0.push(SimulationEvent::Capture {
                player: pending.player,
                cells: result.claimed_cells.len() as u32,
                stolen,
                loop_fill: result.used_loop_fill,
            });
        }
        offset = end;
    }
}

fn resolve_territory_consequences(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    config: Res<GameConfig>,
    mut board: ResMut<BoardGrid>,
    mut displaced: ResMut<DisplacementCredits>,
    mut eliminations: EliminationResources,
    mut query: Query<TerritoryConsequences>,
) {
    let credits = std::mem::take(&mut displaced.0);
    for (victim, killer) in credits {
        if let Some((entity, _, _, mut life, _, _, mut stats, _, trail)) = query
            .iter_mut()
            .find(|(_, c, _, _, _, _, _, _, _)| c.id == victim)
            && life.is_alive()
        {
            if let Some(trail) = trail {
                clear_trail_bits(&mut board, victim, &trail.cells);
                commands.entity(entity).remove::<ActiveTrail>();
            }
            board.clear_owner(victim);
            stats.deaths += 1;
            life.status = LifeStatus::Respawning;
            life.respawn_remaining = config.respawn_delay(stats.deaths);
            eliminations.events.0.push(SimulationEvent::Death {
                victim,
                killer: Some(killer),
                cause: DeathCause::Displaced,
            });
            let match_time = eliminations
                .session
                .as_ref()
                .map_or(0.0, |session| session.elapsed_seconds);
            if let Some(feed) = eliminations.feed.as_deref_mut() {
                feed.push(EliminationRecord {
                    victim,
                    killer: Some(killer),
                    cause: DeathCause::Displaced,
                    match_time,
                });
            }
        }
        if let Some((_, _, _, _, _, _, mut stats, _, _)) = query
            .iter_mut()
            .find(|(_, c, _, _, _, _, _, _, _)| c.id == killer)
        {
            stats.kills += 1;
        }
    }
    for (entity, c, m, life, protection, mut territory, mut stats, last_owned, trail) in &mut query
    {
        let count = board.owner_counts[c.id.index()];
        territory.current_cells = count;
        territory.peak_cells = territory.peak_cells.max(count);
        stats.peak_territory_cells = stats.peak_territory_cells.max(count);
        if life.is_alive() {
            stats.time_alive_seconds += time.delta_secs();
        }
        if life.is_alive()
            && !protection.active()
            && trail.is_none()
            && board
                .world_to_cell(m.position)
                .is_some_and(|cell| !board.owns(cell, c.id))
        {
            let mut new = ActiveTrail::new(c.id, last_owned.0, m.position, m.heading);
            update_trail_raster(&mut board, &mut new, config.trail_width);
            commands.entity(entity).insert(new);
            eliminations
                .events
                .0
                .push(SimulationEvent::TrailStarted { player: c.id });
        }
    }
}

fn check_victory(
    board: Res<BoardGrid>,
    mut session: ResMut<MatchSession>,
    mut events: ResMut<SimulationEvents>,
    mut next: ResMut<NextState<AppState>>,
) {
    if let Some((index, _)) = board
        .owner_counts
        .iter()
        .enumerate()
        .find(|(_, count)| **count == board.playable_cells)
    {
        let winner = CompetitorId(index as u8);
        session.phase = MatchPhase::Finished;
        session.winner = Some(winner);
        session.result_hold_remaining = 3.0;
        events.0.push(SimulationEvent::Victory { winner });
        next.set(AppState::GameOver);
    }
}

fn choose_respawn(
    board: &BoardGrid,
    id: CompetitorId,
    seed: u64,
    living: &[(CompetitorId, Vec2)],
    trail_points: &[Vec2],
    reserved: &[Vec2],
    config: &GameConfig,
) -> Vec2 {
    let mut rng = DeterministicRng::new(seed ^ (id.0 as u64 + 1).wrapping_mul(0x9e37_79b9));
    let leader = living.first().map(|p| p.1).unwrap_or(Vec2::ZERO);
    let candidates: Vec<usize> = (0..board.len())
        .filter(|&i| {
            board.field_mask[i] && board.signed_distance[i] >= config.spawn_boundary_clearance
        })
        .collect();
    let mut best = None;
    for relax in [1.0, 0.65, 0.0] {
        for _ in 0..256.min(candidates.len()) {
            let i = candidates[rng.index(candidates.len())];
            let p = board.cell_center(board.cell(i));
            let cube = living
                .iter()
                .map(|(_, q)| q.distance(p))
                .fold(f32::INFINITY, f32::min);
            let trail = trail_points
                .iter()
                .map(|q| q.distance(p))
                .fold(f32::INFINITY, f32::min);
            if cube < config.spawn_cube_clearance * relax
                || trail < config.spawn_trail_clearance * relax
                || reserved.iter().any(|q| {
                    q.distance(p)
                        < config
                            .spawn_cube_clearance
                            .max(config.starting_territory_radius * 2.0)
                })
            {
                continue;
            }
            let unclaimed =
                spawn_disk_unclaimed_ratio(board, p, config.starting_territory_radius) * 100.0;
            let score = unclaimed
                + cube.min(30.0)
                + trail.min(20.0)
                + p.distance(leader) * 0.5
                + board.signed_distance[i];
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((p, score));
            }
        }
        if best.is_some() {
            break;
        }
    }
    best.map(|x| x.0).unwrap_or_else(|| {
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

fn spawn_disk_unclaimed_ratio(board: &BoardGrid, center: Vec2, radius: f32) -> f32 {
    let radius_squared = radius * radius;
    let mut playable = 0_u32;
    let mut unclaimed = 0_u32;
    let Some(min) = board.world_to_cell(center - Vec2::splat(radius)) else {
        return 0.0;
    };
    let Some(max) = board.world_to_cell(center + Vec2::splat(radius)) else {
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

fn advance_respawns(
    time: Res<Time<Fixed>>,
    config: Res<GameConfig>,
    mut board: ResMut<BoardGrid>,
    session: Res<MatchSession>,
    mut events: ResMut<SimulationEvents>,
    mut displacement_credits: ResMut<DisplacementCredits>,
    mut queries: ParamSet<(Query<RespawnSnapshot>, Query<RespawnControl>)>,
) {
    if session.phase != MatchPhase::Running {
        return;
    }
    let mut living: Vec<_> = queries
        .p0()
        .iter()
        .filter(|(_, _, l, _)| l.is_alive())
        .map(|(c, m, _, _)| (c.id, m.position))
        .collect();
    living.sort_by(|(a, _), (b, _)| {
        board.owner_counts[b.index()]
            .cmp(&board.owner_counts[a.index()])
            .then_with(|| a.cmp(b))
    });
    let trail_points: Vec<_> = queries
        .p0()
        .iter()
        .filter_map(|(_, _, _, t)| t)
        .flat_map(|t| t.points.iter().copied())
        .collect();
    let mut reserved = Vec::new();
    for (c, mut motion, mut life, mut protection, mut territory, mut stats) in
        queries.p1().iter_mut()
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
            c.id,
            session.seed ^ stats.deaths as u64,
            &living,
            &trail_points,
            &reserved,
            &config,
        );
        reserved.push(position);
        claim_respawn_seed(
            &mut board,
            position,
            config.starting_territory_radius,
            c.id,
            &mut displacement_credits,
        );
        motion.position = position;
        motion.previous_position = position;
        motion.heading = (-position).try_normalize().unwrap_or(Vec2::Y);
        life.status = LifeStatus::Alive;
        protection.remaining = config.spawn_protection_seconds;
        protection.elapsed = 0.0;
        territory.current_cells = board.owner_counts[c.id.index()];
        territory.peak_cells = territory.peak_cells.max(territory.current_cells);
        stats.peak_territory_cells = stats.peak_territory_cells.max(territory.current_cells);
        events.0.push(SimulationEvent::Respawn { player: c.id });
    }
}

fn claim_respawn_seed(
    board: &mut BoardGrid,
    position: Vec2,
    radius: f32,
    respawning: CompetitorId,
    credits: &mut DisplacementCredits,
) {
    let previous_counts = board.owner_counts;
    board.claim_disk(position, radius, respawning);
    for (index, previous) in previous_counts.into_iter().enumerate() {
        let victim = CompetitorId(index as u8);
        if victim != respawning && previous > 0 && board.owner_counts[index] == 0 {
            credits.0.push((victim, respawning));
        }
    }
}

fn update_rankings(
    board: Res<BoardGrid>,
    mut rankings: ResMut<Rankings>,
    mut events: ResMut<SimulationEvents>,
    query: Query<(&Competitor, &LifeState, &MatchStatistics)>,
) {
    let old: Vec<_> = rankings.entries.iter().map(|e| e.id).collect();
    let mut data: Vec<_> = query
        .iter()
        .map(|(c, l, s)| {
            (
                c.id,
                board.owner_counts[c.id.index()],
                l.is_alive(),
                s.kills,
            )
        })
        .collect();
    data.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| b.3.cmp(&a.3))
            .then_with(|| a.0.cmp(&b.0))
    });
    rankings.entries = data
        .into_iter()
        .enumerate()
        .map(|(i, (id, cells, alive, kills))| RankingEntry {
            id,
            rank: (i + 1) as u8,
            territory_cells: cells,
            territory_percent: if board.playable_cells == 0 {
                0.0
            } else {
                cells as f32 * 100.0 / board.playable_cells as f32
            },
            alive,
            kills,
        })
        .collect();
    if old != rankings.entries.iter().map(|e| e.id).collect::<Vec<_>>() {
        events.0.push(SimulationEvent::RankingChanged);
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
