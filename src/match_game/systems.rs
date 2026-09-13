use bevy::{prelude::*, time::Fixed};

use crate::{
    board::BoardGrid,
    combat::{CollisionBody, CollisionTuning, CollisionWorkspace},
    config::GameConfig,
    ids::{CompetitorId, MAX_COMPETITORS},
    movement::{CompetitorMotion, advance_motion},
    npc::{NpcController, NpcEventQueue},
    territory_map::TerritoryMap,
    trail::{ActiveTrail, update_trail_raster},
};

use super::SteeringIntent;
use super::capture_systems::{detect_closures, resolve_captures};
use super::lifecycle::advance_countdown;
use super::model::*;
use super::npc_systems::npc_think;
use super::replay::PendingCommands;
use super::respawn::advance_respawns;
#[cfg(test)]
use super::respawn::{claim_respawn_seed, reset_respawn_anchor};

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

pub struct SimulationPlugin;

fn presentation_is_ready(generation: Res<MatchGeneration>, ready: Res<PresentationReady>) -> bool {
    ready.0 == Some(generation.0)
}

fn match_is_running(
    session: Res<MatchSession>,
    paused: Res<SimulationPaused>,
    generation: Res<MatchGeneration>,
    ready: Res<PresentationReady>,
) -> bool {
    !paused.0 && session.phase == MatchPhase::Running && presentation_is_ready(generation, ready)
}

impl Plugin for SimulationPlugin {
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
            .init_resource::<NpcEventQueue>()
            .init_resource::<PendingDeaths>()
            .init_resource::<PendingCaptures>()
            .init_resource::<DisplacementCredits>()
            .init_resource::<MatchGeneration>()
            .init_resource::<PresentationReady>()
            .init_resource::<SimulationPaused>()
            .init_resource::<SimulationClock>()
            .init_resource::<PendingCommands>()
            .init_resource::<crate::match_game::rules::MatchRules>()
            .init_resource::<Time<Fixed>>()
            .init_resource::<Time>()
            .configure_sets(FixedUpdate, sets.run_if(match_is_running))
            .add_systems(Startup, configure_fixed_timestep)
            .add_systems(
                FixedUpdate,
                (super::lifecycle::transition_when_ready, advance_countdown)
                    .chain()
                    .before(MatchSystemSet::PollInput),
            )
            .add_systems(
                FixedUpdate,
                (
                    super::replay::consume_commands.in_set(MatchSystemSet::BuildSteeringIntent),
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
                    deliver_npc_events.after(MatchSystemSet::UpdateRankings),
                )
                    .run_if(match_is_running),
            );
    }
}

fn configure_fixed_timestep(config: Res<GameConfig>, mut fixed_time: ResMut<Time<Fixed>>) {
    fixed_time.set_timestep_hz(config.fixed_hz);
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

#[allow(clippy::too_many_arguments)]
fn extend_trails(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut board: ResMut<BoardGrid>,
    territory: Res<TerritoryMap>,
    mut events: ResMut<SimulationEvents>,
    mut query: Query<TrailExtension>,
    mut order: Local<Vec<(CompetitorId, Entity)>>,
) {
    order.clear();
    order.extend(
        query
            .iter()
            .map(|(entity, competitor, ..)| (competitor.id, entity)),
    );
    order.sort_unstable_by_key(|(id, _)| *id);
    for &(_, entity) in order.iter() {
        let Ok((entity, competitor, motion, life, protection, mut last_owned, trail)) =
            query.get_mut(entity)
        else {
            continue;
        };
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

#[allow(clippy::too_many_arguments)]
fn resolve_deaths(
    mut commands: Commands,
    config: Res<GameConfig>,
    rules: Option<Res<super::rules::MatchRules>>,
    mut board: ResMut<BoardGrid>,
    mut territory: ResMut<TerritoryMap>,
    mut pending: ResMut<PendingDeaths>,
    mut eliminations: super::outcomes::EliminationResources,
    mut query: Query<super::outcomes::EliminationQuery>,
) {
    let rules = rules
        .as_deref()
        .cloned()
        .unwrap_or_else(|| super::rules::MatchRules::from(config.as_ref()));
    let intents = std::mem::take(&mut pending.0);
    super::outcomes::resolve_eliminations(
        intents.iter().copied().map(|intent| {
            super::outcomes::EliminationOutcome::trail_collision(intent.victim, intent.killer)
        }),
        &mut commands,
        &mut board,
        &mut territory,
        &rules,
        &mut eliminations,
        &mut query,
    );
    pending.0 = intents;
    pending.0.clear();
}

#[allow(clippy::too_many_arguments)]
fn resolve_territory_consequences(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    config: Res<GameConfig>,
    rules: Option<Res<super::rules::MatchRules>>,
    mut board: ResMut<BoardGrid>,
    mut territory_map: ResMut<TerritoryMap>,
    mut displaced: ResMut<DisplacementCredits>,
    mut eliminations: super::outcomes::EliminationResources,
    mut queries: ParamSet<(
        Query<TerritoryConsequences>,
        Query<super::outcomes::EliminationQuery>,
    )>,
    mut order: Local<Vec<(CompetitorId, Entity)>>,
) {
    let rules = rules
        .as_deref()
        .cloned()
        .unwrap_or_else(|| super::rules::MatchRules::from(config.as_ref()));
    let credits = std::mem::take(&mut displaced.0);
    {
        let mut query = queries.p1();
        super::outcomes::resolve_eliminations(
            credits.iter().copied().map(|(victim, killer)| {
                super::outcomes::EliminationOutcome::displaced(victim, killer)
            }),
            &mut commands,
            &mut board,
            &mut territory_map,
            &rules,
            &mut eliminations,
            &mut query,
        );
    }
    displaced.0 = credits;
    displaced.0.clear();
    let mut query = queries.p0();
    order.clear();
    order.extend(
        query
            .iter()
            .map(|(entity, competitor, ..)| (competitor.id, entity)),
    );
    order.sort_unstable_by_key(|(id, _)| *id);
    for &(_, entity) in order.iter() {
        let Ok((entity, c, m, life, protection, mut territory, mut stats, last_owned, trail)) =
            query.get_mut(entity)
        else {
            continue;
        };
        // Exact polygon areas only change on ownership commits. Do not walk
        // every contour at 60 Hz merely to repeat the same score.
        if territory_map.is_changed() {
            let area = territory_map.area(c.id);
            territory.current_area = area;
            territory.peak_area = territory.peak_area.max(area);
            stats.peak_territory_area = stats.peak_territory_area.max(area);
        }
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

fn deliver_npc_events(
    mut queue: ResMut<NpcEventQueue>,
    mut controllers: Query<(&Competitor, &mut NpcController)>,
) {
    for message in queue.0.drain(..) {
        if let Some((_, mut controller)) = controllers
            .iter_mut()
            .find(|(competitor, _)| competitor.id == message.recipient)
        {
            controller.on_event(message.event, message.tick);
        }
    }
}

fn check_victory(
    config: Res<GameConfig>,
    rules: Option<Res<super::rules::MatchRules>>,
    territory: Res<TerritoryMap>,
    mut session: ResMut<MatchSession>,
    mut events: ResMut<SimulationEvents>,
) {
    if session.phase != MatchPhase::Running {
        return;
    }
    let rules = rules
        .as_deref()
        .cloned()
        .unwrap_or_else(|| super::rules::MatchRules::from(config.as_ref()));
    if !rules.victory_enabled {
        return;
    }
    let winner = territory
        .territories()
        .iter()
        .enumerate()
        .find(|(index, _)| {
            territory.reaches_victory_threshold(
                CompetitorId(*index as u8),
                rules.victory_threshold_percent,
            )
        })
        .map(|(index, _)| CompetitorId(index as u8));
    if let Some(winner) = winner {
        session.phase = MatchPhase::Finished;
        session.winner = Some(winner);
        events.0.push(SimulationEvent::Victory { winner });
    }
}

fn update_rankings(
    territory_map: Res<TerritoryMap>,
    mut rankings: ResMut<Rankings>,
    mut events: ResMut<SimulationEvents>,
    query: Query<(&Competitor, &LifeState, &MatchStatistics)>,
    mut scratch: Local<Vec<(CompetitorId, f32, bool, u32)>>,
) {
    // Alive-time statistics change every tick, so Changed<MatchStatistics>
    // would invalidate this cache continuously. Compare only ranking inputs.
    if !territory_map.is_changed()
        && rankings.entries.len() == query.iter().len()
        && query.iter().all(|(competitor, life, stats)| {
            rankings
                .rank_of(competitor.id)
                .is_some_and(|entry| entry.alive == life.is_alive() && entry.kills == stats.kills)
        })
    {
        return;
    }
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
            territory_percent: if territory_map.arena_area() <= 0.0 {
                0.0
            } else {
                area * 100.0 / territory_map.arena_area()
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

#[cfg(test)]
#[path = "ranking_cache_tests.rs"]
mod ranking_cache_tests;
