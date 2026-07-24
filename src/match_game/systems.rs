use bevy::{ecs::system::SystemParam, prelude::*, time::Fixed};

use crate::{
    app_state::AppState,
    board::BoardGrid,
    combat::{CollisionBody, CollisionTuning, CollisionWorkspace},
    config::GameConfig,
    ids::{CompetitorId, MAX_COMPETITORS},
    input::{ControlSource, SteeringIntent},
    movement::{CompetitorMotion, advance_motion},
    npc::{
        BoardQuery, NpcContext, NpcController, NpcSelfState, PerceivedCompetitor, PerceivedTrail,
        RankingSnapshot,
    },
    territory_map::TerritoryMap,
    trail::{ActiveTrail, clear_trail_bits, update_trail_raster},
};

use super::capture_systems::{detect_closures, resolve_captures};
use super::lifecycle::{
    AttractSeedSequence, MatchLoadingFrames, advance_countdown, advance_result_hold,
    begin_attract_match, begin_from_lobby, cleanup_match, finish_match_loading,
};
use super::model::*;
use super::respawn::advance_respawns;
#[cfg(test)]
use super::respawn::{claim_respawn_seed, reset_respawn_anchor};

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
#[derive(SystemParam)]
struct EliminationResources<'w> {
    events: ResMut<'w, SimulationEvents>,
    session: Option<Res<'w, MatchSession>>,
    feed: Option<ResMut<'w, EliminationFeed>>,
}

pub struct MatchPlugin;

fn match_is_counting_down(state: Res<State<AppState>>, session: Res<MatchSession>) -> bool {
    session.phase == MatchPhase::Countdown
        && session.purpose == MatchPurpose::Playable
        && *state.get() == AppState::Countdown
}

fn match_is_running(state: Res<State<AppState>>, session: Res<MatchSession>) -> bool {
    simulation_is_active(state.get(), &session)
}

fn simulation_is_active(state: &AppState, session: &MatchSession) -> bool {
    session.phase == MatchPhase::Running
        && matches!(
            (state, session.purpose),
            (AppState::Playing, MatchPurpose::Playable) | (AppState::Home, MatchPurpose::Attract)
        )
}

fn match_is_attract(session: Res<MatchSession>) -> bool {
    session.purpose == MatchPurpose::Attract
}

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
            .init_resource::<TerritoryMap>()
            .init_resource::<MatchSession>()
            .init_resource::<Rankings>()
            .init_resource::<SimulationEvents>()
            .init_resource::<EliminationFeed>()
            .init_resource::<PendingDeaths>()
            .init_resource::<PendingCaptures>()
            .init_resource::<DisplacementCredits>()
            .init_resource::<MatchLoadingFrames>()
            .init_resource::<AttractSeedSequence>()
            .init_resource::<Time<Fixed>>()
            .configure_sets(FixedUpdate, sets)
            .add_systems(Startup, configure_fixed_timestep)
            .add_systems(OnEnter(AppState::MatchLoading), begin_from_lobby)
            .add_systems(OnEnter(AppState::Home), begin_attract_match)
            .add_systems(
                OnExit(AppState::Home),
                cleanup_match.run_if(match_is_attract),
            )
            .add_systems(
                Update,
                finish_match_loading.run_if(in_state(AppState::MatchLoading)),
            )
            .add_systems(OnEnter(AppState::Lobby), cleanup_match)
            .add_systems(
                FixedUpdate,
                advance_countdown.run_if(match_is_counting_down),
            )
            .add_systems(
                FixedUpdate,
                (
                    npc_think.in_set(MatchSystemSet::NpcThink),
                    move_competitors.in_set(MatchSystemSet::MoveCompetitors),
                    extend_trails.in_set(MatchSystemSet::ExtendTrails),
                    detect_trail_collisions.in_set(MatchSystemSet::DetectTrailCollisions),
                    resolve_deaths.in_set(MatchSystemSet::ResolveDeaths),
                    detect_closures.in_set(MatchSystemSet::DetectClosures),
                    resolve_captures.in_set(MatchSystemSet::ResolveCaptures),
                    resolve_territory_consequences
                        .in_set(MatchSystemSet::ResolveTerritoryConsequences),
                    check_victory.in_set(MatchSystemSet::CheckVictory),
                    advance_respawns.in_set(MatchSystemSet::AdvanceRespawns),
                    update_rankings.in_set(MatchSystemSet::UpdateRankings),
                )
                    .run_if(match_is_running),
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

#[allow(clippy::too_many_arguments)]
fn npc_think(
    time: Res<Time<Fixed>>,
    board: Res<BoardGrid>,
    territory: Res<TerritoryMap>,
    session: Res<MatchSession>,
    rankings: Res<Rankings>,
    mut queries: ParamSet<(Query<NpcSnapshot>, Query<NpcControl>)>,
    mut people: Local<Vec<PerceivedCompetitor>>,
    mut trail_perceptions: Local<[Vec<PerceivedTrail>; MAX_COMPETITORS]>,
    mut ranking: Local<RankingSnapshot>,
    mut nearby_people: Local<Vec<PerceivedCompetitor>>,
    mut nearby_trails: Local<Vec<PerceivedTrail>>,
) {
    let should_build_snapshot = queries
        .p1()
        .iter()
        .any(|(_, _, life, _, _, controller, _)| {
            life.is_alive() && controller.think_remaining <= time.delta_secs()
        });
    if !should_build_snapshot {
        for (_, _, life, _, _, mut controller, _) in queries.p1().iter_mut() {
            if life.is_alive() {
                controller.think_remaining -= time.delta_secs();
            }
        }
        return;
    }
    people.clear();
    people.extend(
        queries
            .p0()
            .iter()
            .map(|(c, m, l, t, _)| PerceivedCompetitor {
                id: c.id,
                position: m.position,
                alive: l.is_alive(),
                territory_cells: t.current_cells,
                territory_area: t.current_area,
            }),
    );
    trail_perceptions
        .iter_mut()
        .for_each(|perceptions| perceptions.clear());
    {
        let snapshots = queries.p0();
        for viewer in people.iter() {
            for (owner, _, _, _, trail) in snapshots.iter() {
                let Some(trail) = trail.filter(|_| owner.id != viewer.id) else {
                    continue;
                };
                let nearest = trail
                    .points
                    .iter()
                    .copied()
                    .chain(std::iter::once(trail.head))
                    .min_by(|a, b| {
                        a.distance_squared(viewer.position)
                            .total_cmp(&b.distance_squared(viewer.position))
                    });
                if let Some(nearest_point) = nearest {
                    trail_perceptions[viewer.id.index()].push(PerceivedTrail {
                        owner: owner.id,
                        nearest_point,
                        distance: nearest_point.distance(viewer.position),
                    });
                }
            }
        }
    }
    ranking.ordered.clear();
    ranking
        .ordered
        .extend(rankings.entries.iter().map(|e| e.id));
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
        nearby_people.clear();
        nearby_people.extend(
            people.iter().copied().filter(|p| {
                p.id != competitor.id && p.position.distance(motion.position) <= radius
            }),
        );
        nearby_trails.clear();
        nearby_trails.extend(
            trail_perceptions[competitor.id.index()]
                .iter()
                .copied()
                .filter(|t| t.distance <= radius),
        );
        let query = BoardQuery::new(&board, &territory, competitor.id);
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
    territory: Res<TerritoryMap>,
    mut session: ResMut<MatchSession>,
    mut query: Query<(
        &Competitor,
        &mut CompetitorMotion,
        &SteeringIntent,
        &LifeState,
        &MatchStatistics,
        &mut SpawnProtection,
    )>,
) {
    let dt = time.delta_secs();
    session.elapsed_seconds += dt;
    for (competitor, mut motion, intent, life, stats, mut protection) in &mut query {
        if !life.is_alive() {
            continue;
        }
        let desired = (intent.magnitude > 0.0).then_some(intent.desired_direction);
        advance_motion(
            &mut motion,
            desired,
            &territory,
            &config,
            config.player_speed_for_kills(stats.kills),
            dt,
        );
        advance_spawn_protection(
            &mut protection,
            competitor.id,
            motion.position,
            &territory,
            &config,
            dt,
        );
    }
}

fn advance_spawn_protection(
    protection: &mut SpawnProtection,
    competitor: CompetitorId,
    position: Vec2,
    territory: &TerritoryMap,
    config: &GameConfig,
    delta_seconds: f32,
) {
    if !protection.active() {
        return;
    }
    protection.elapsed += delta_seconds;
    protection.remaining = (protection.remaining - delta_seconds).max(0.0);
    if protection.elapsed >= config.spawn_protection_minimum_seconds
        && !territory.owns(position, competitor)
    {
        protection.remaining = 0.0;
    }
}

fn extend_trails(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut board: ResMut<BoardGrid>,
    territory: Res<TerritoryMap>,
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
        let currently_owned = territory.owns(motion.position, competitor.id);
        if let Some(mut trail) = trail {
            trail.append(
                motion.position,
                motion.heading,
                config.trail_sample_distance,
                config.trail_sample_angle_radians,
            );
            update_trail_raster(&mut board, &mut trail, config.trail_width);
        } else if currently_owned {
            last_owned.0 = current;
        } else {
            let from = if territory.owns(motion.previous_position, competitor.id) {
                motion.previous_position
            } else {
                board.cell_center(last_owned.0)
            };
            let boundary = territory.boundary_crossing(competitor.id, from, motion.position);
            let mut trail = ActiveTrail::new(competitor.id, last_owned.0, boundary, motion.heading);
            trail.append(
                motion.position,
                motion.heading,
                config.trail_sample_distance,
                config.trail_sample_angle_radians,
            );
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
    board: Res<BoardGrid>,
    bodies: Query<(&Competitor, &CompetitorMotion, &LifeState, &SpawnProtection)>,
    trails: Query<&ActiveTrail>,
    mut pending: ResMut<PendingDeaths>,
    mut snapshots: Local<Vec<CollisionBody>>,
    mut collision_workspace: Local<CollisionWorkspace>,
) {
    snapshots.clear();
    snapshots.extend(
        bodies
            .iter()
            .filter(|(_, _, l, _)| l.is_alive())
            .map(|(c, m, _, p)| CollisionBody {
                id: c.id,
                previous: m.previous_position,
                current: m.position,
                protected: p.active(),
            }),
    );
    let mut trail_slots = [None; MAX_COMPETITORS];
    for trail in &trails {
        trail_slots[trail.owner.index()] = Some(trail);
    }
    crate::combat::collect_collision_intents_into(
        &board,
        &snapshots,
        &trail_slots,
        CollisionTuning {
            collision_radius: config.collision_radius,
            trail_width: config.trail_width,
            self_exclusion: config.self_trail_exclusion_distance,
        },
        &mut pending.0,
        &mut collision_workspace,
    );
}

fn resolve_deaths(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut board: ResMut<BoardGrid>,
    mut territory: ResMut<TerritoryMap>,
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
    for intent in intents.iter().copied() {
        let mut eliminated = false;
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
            let changed = territory.territories[competitor.id.index()].clone();
            if territory.clear_owner(competitor.id) > 1e-5 {
                territory.refresh_sample_cache(&mut board, &changed);
            }
            stats.deaths += 1;
            stats.reset_kill_streak();
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
            eliminated = true;
        }
        if eliminated
            && let Some(killer) = intent.killer
            && let Some((_, _, _, mut stats, _)) =
                query.iter_mut().find(|(_, c, _, _, _)| c.id == killer)
        {
            credit_kill(killer, &mut stats, &mut eliminations.events);
        }
    }
    pending.0 = intents;
    pending.0.clear();
}

#[allow(clippy::too_many_arguments)]
fn resolve_territory_consequences(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    config: Res<GameConfig>,
    mut board: ResMut<BoardGrid>,
    mut territory_map: ResMut<TerritoryMap>,
    mut displaced: ResMut<DisplacementCredits>,
    mut eliminations: EliminationResources,
    mut query: Query<TerritoryConsequences>,
) {
    let credits = std::mem::take(&mut displaced.0);
    for (victim, killer) in credits.iter().copied() {
        let mut eliminated = false;
        if let Some((entity, _, _, mut life, _, _, mut stats, _, trail)) = query
            .iter_mut()
            .find(|(_, c, _, _, _, _, _, _, _)| c.id == victim)
            && life.is_alive()
        {
            if let Some(trail) = trail {
                clear_trail_bits(&mut board, victim, &trail.cells);
                commands.entity(entity).remove::<ActiveTrail>();
            }
            let changed = territory_map.territories[victim.index()].clone();
            if territory_map.clear_owner(victim) > 1e-5 {
                territory_map.refresh_sample_cache(&mut board, &changed);
            }
            stats.deaths += 1;
            stats.reset_kill_streak();
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
            eliminated = true;
        }
        if eliminated
            && let Some((_, _, _, _, _, _, mut stats, _, _)) = query
                .iter_mut()
                .find(|(_, c, _, _, _, _, _, _, _)| c.id == killer)
        {
            credit_kill(killer, &mut stats, &mut eliminations.events);
        }
    }
    displaced.0 = credits;
    displaced.0.clear();
    for (entity, c, m, life, protection, mut territory, mut stats, last_owned, trail) in &mut query
    {
        let count = board.owner_counts[c.id.index()];
        let area = territory_map.area(c.id);
        territory.current_area = area;
        territory.peak_area = territory.peak_area.max(area);
        territory.current_cells = count;
        territory.peak_cells = territory.peak_cells.max(count);
        stats.peak_territory_area = stats.peak_territory_area.max(area);
        stats.peak_territory_cells = stats.peak_territory_cells.max(count);
        if life.is_alive() {
            stats.time_alive_seconds += time.delta_secs();
        }
        if life.is_alive()
            && !protection.active()
            && trail.is_none()
            && !territory_map.owns(m.position, c.id)
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

fn credit_kill(killer: CompetitorId, stats: &mut MatchStatistics, events: &mut SimulationEvents) {
    let progress = stats.record_kill();
    events.0.push(SimulationEvent::Kill { killer, progress });
}

fn check_victory(
    config: Res<GameConfig>,
    territory: Res<TerritoryMap>,
    mut session: ResMut<MatchSession>,
    mut events: ResMut<SimulationEvents>,
    mut next: ResMut<NextState<AppState>>,
) {
    if session.purpose == MatchPurpose::Attract || session.phase == MatchPhase::Finished {
        return;
    }
    let winner = territory
        .territories
        .iter()
        .enumerate()
        .find(|(index, _)| {
            territory.reaches_victory_threshold(
                CompetitorId(*index as u8),
                config.victory_territory_percent,
            )
        })
        .map(|(index, _)| CompetitorId(index as u8));
    if let Some(winner) = winner {
        session.phase = MatchPhase::Finished;
        session.winner = Some(winner);
        session.result_hold_remaining = 3.0;
        events.0.push(SimulationEvent::Victory { winner });
        next.set(AppState::GameOver);
    }
}

fn update_rankings(
    board: Res<BoardGrid>,
    territory_map: Res<TerritoryMap>,
    mut rankings: ResMut<Rankings>,
    mut events: ResMut<SimulationEvents>,
    query: Query<(&Competitor, &LifeState, &MatchStatistics)>,
    mut scratch: Local<Vec<(CompetitorId, f32, bool, u32)>>,
) {
    scratch.clear();
    scratch.extend(
        query
            .iter()
            .map(|(c, l, s)| (c.id, territory_map.area(c.id), l.is_alive(), s.kills)),
    );
    scratch.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| b.3.cmp(&a.3))
            .then_with(|| a.0.cmp(&b.0))
    });
    let order_changed = rankings.entries.len() != scratch.len()
        || rankings
            .entries
            .iter()
            .zip(scratch.iter())
            .any(|(old, new)| old.id != new.0);
    rankings.entries.clear();
    rankings.entries.reserve(scratch.len());
    for (i, (id, area, alive, kills)) in scratch.iter().copied().enumerate() {
        rankings.entries.push(RankingEntry {
            id,
            rank: (i + 1) as u8,
            territory_area: area,
            territory_cells: board.owner_counts[id.index()],
            territory_percent: if territory_map.arena_area <= 0.0 {
                0.0
            } else {
                area * 100.0 / territory_map.arena_area
            },
            alive,
            kills,
        });
    }
    if order_changed {
        events.0.push(SimulationEvent::RankingChanged);
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
