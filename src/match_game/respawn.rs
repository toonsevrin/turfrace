use bevy::{prelude::*, time::Fixed};

use crate::{
    board::{BoardGrid, point_segment_distance},
    config::GameConfig,
    ids::CompetitorId,
    movement::CompetitorMotion,
    npc::{NpcEvent, NpcEventMessage, NpcEventQueue},
    territory_map::TerritoryMap,
    trail::ActiveTrail,
};

use super::{
    model::{
        Competitor, DisplacementCredits, LastOwnedCell, LifeState, LifeStatus, MatchPhase,
        MatchSession, MatchStatistics, RespawnPlan, SimulationEvent, SimulationEvents,
        SpawnProtection, TerritoryRecord,
    },
    rules::MatchRules,
};

const MIN_RESPAWN_WARNING_SECONDS: f32 = 5.0;

type RespawnSnapshot = (
    Entity,
    &'static Competitor,
    &'static CompetitorMotion,
    &'static LifeState,
    Option<&'static ActiveTrail>,
    Option<&'static RespawnPlan>,
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
    Option<&'static RespawnPlan>,
);

#[derive(Clone, Copy, Debug, PartialEq)]
struct RespawnReservation {
    entity: Entity,
    center: Vec2,
    radius: f32,
    /// Polygon booleans are only repeated when ownership geometry changes.
    neutral_revision: u64,
    neutral: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RespawnWaiter {
    entity: Entity,
    ticket: u64,
    cursor: usize,
    stride: usize,
}

#[derive(Resource, Debug, Default)]
pub(super) struct RespawnScheduler {
    reservations: Vec<RespawnReservation>,
    waiters: Vec<RespawnWaiter>,
    retry_remaining: f32,
    next_ticket: u64,
}

/// Terminal matches cannot run the normal respawn set, but any plan created
/// earlier in the winning tick must still be removed. Keep this outside the
/// running gate so victory cannot leave stale gameplay or presentation state.
pub(super) fn cleanup_terminal_respawns(
    mut commands: Commands,
    session: Res<MatchSession>,
    mut scheduler: ResMut<RespawnScheduler>,
    mut query: Query<(Entity, &mut LifeState, Option<&RespawnPlan>)>,
) {
    if session.phase == MatchPhase::Running {
        return;
    }
    *scheduler = RespawnScheduler::default();
    for (entity, mut life, plan) in &mut query {
        if life.status == LifeStatus::Respawning {
            life.status = LifeStatus::Eliminated;
            life.respawn_remaining = 0.0;
        }
        if plan.is_some() {
            commands.entity(entity).remove::<RespawnPlan>();
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn advance_respawns(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    config: Res<GameConfig>,
    rules: Option<Res<MatchRules>>,
    board: Res<BoardGrid>,
    mut territory_map: ResMut<TerritoryMap>,
    session: Res<MatchSession>,
    clock: Option<Res<super::model::SimulationClock>>,
    mut events: ResMut<SimulationEvents>,
    mut npc_events: ResMut<NpcEventQueue>,
    mut _displacement_credits: ResMut<DisplacementCredits>,
    mut scheduler: ResMut<RespawnScheduler>,
    mut queries: ParamSet<(Query<RespawnSnapshot>, Query<RespawnControl>)>,
    mut living_scratch: Local<Vec<(CompetitorId, Vec2)>>,
    mut trail_scratch: Local<Vec<(Vec2, Vec2)>>,
    mut respawning_scratch: Local<Vec<(CompetitorId, Entity, bool)>>,
) {
    if session.phase != MatchPhase::Running {
        return;
    }
    let rules = rules
        .as_deref()
        .cloned()
        .unwrap_or_else(|| MatchRules::from(config.as_ref()));
    if !rules.respawn_enabled {
        *scheduler = RespawnScheduler::default();
        let mut controls = queries.p1();
        for (entity, _, _, mut life, _, _, _, _, _, plan) in &mut controls {
            if life.status == LifeStatus::Respawning {
                life.status = LifeStatus::Eliminated;
                life.respawn_remaining = 0.0;
            }
            if plan.is_some() {
                commands.entity(entity).remove::<RespawnPlan>();
            }
        }
        return;
    }

    living_scratch.clear();
    trail_scratch.clear();
    respawning_scratch.clear();
    {
        let snapshot = queries.p0();
        for (entity, competitor, motion, life, _, plan) in &snapshot {
            if life.is_alive() {
                living_scratch.push((competitor.id, motion.position));
            }
            if life.status == LifeStatus::Respawning {
                respawning_scratch.push((competitor.id, entity, plan.is_some()));
            }
        }
        if !respawning_scratch.is_empty() {
            for (_, _, _, life, trail, _) in &snapshot {
                if life.is_alive()
                    && let Some(trail) = trail
                {
                    trail_scratch.extend(
                        (0..trail.segment_count()).filter_map(|index| trail.segment(index)),
                    );
                }
            }
        }
    }
    living_scratch.sort_by_key(|(id, _)| *id);
    respawning_scratch.sort_unstable_by_key(|(id, _, _)| *id);

    let active_entities: Vec<_> = respawning_scratch
        .iter()
        .map(|(_, entity, _)| *entity)
        .collect();
    scheduler
        .reservations
        .retain(|reservation| active_entities.contains(&reservation.entity));
    // A plan without its reservation is stale (for example after a reset or
    // external recovery). Never leave presentation showing a site we no
    // longer own; force a fresh warning on the next successful reservation.
    let stale_plans: Vec<_> = respawning_scratch
        .iter()
        .filter(|(_, entity, has_plan)| {
            *has_plan
                && !scheduler
                    .reservations
                    .iter()
                    .any(|reservation| reservation.entity == *entity)
        })
        .map(|(_, entity, _)| *entity)
        .collect();
    if !stale_plans.is_empty() {
        let mut controls = queries.p1();
        for &entity in &stale_plans {
            if let Ok((_, _, _, mut life, _, _, _, _, _, _)) = controls.get_mut(entity) {
                life.respawn_remaining = 0.0;
            }
            commands.entity(entity).remove::<RespawnPlan>();
        }
    }

    // A waiter survives every failed attempt. Tickets provide oldest-waiter
    // ordering while still making each retry deterministic.
    for &(id, entity, _) in &respawning_scratch {
        if !scheduler
            .reservations
            .iter()
            .any(|reservation| reservation.entity == entity)
            && !scheduler
                .waiters
                .iter()
                .any(|waiter| waiter.entity == entity)
        {
            let ticket = scheduler.next_ticket;
            scheduler.next_ticket = scheduler.next_ticket.wrapping_add(1);
            scheduler.waiters.push(RespawnWaiter {
                entity,
                ticket,
                cursor: 0,
                stride: coprime_stride(
                    board.spawn_candidates.len(),
                    session.field_seed ^ u64::from(id.0),
                ),
            });
        }
    }
    scheduler
        .waiters
        .retain(|waiter| active_entities.contains(&waiter.entity));
    scheduler.waiters.sort_by_key(|waiter| waiter.ticket);

    // Invalidation is deliberately checked every tick, but its expensive
    // neutral boolean is cached until the territory revision changes.
    let mut canceled = Vec::new();
    let reservation_snapshot = scheduler.reservations.clone();
    for reservation in &mut scheduler.reservations {
        if reservation.neutral_revision != territory_map.revision() {
            reservation.neutral =
                territory_map.is_neutral_seed_site(reservation.center, reservation.radius);
            reservation.neutral_revision = territory_map.revision();
        }
        let safety = RespawnSafetyContext {
            board: &board,
            territory: &territory_map,
            living: &living_scratch,
            trails: &trail_scratch,
            reserved: &reservation_snapshot,
            config: &config,
        };
        if !reservation.neutral
            || !safety.is_valid(
                reservation.center,
                reservation.radius,
                Some(reservation.entity),
                Some(reservation.neutral),
            )
        {
            canceled.push(reservation.entity);
        }
    }
    if !canceled.is_empty() {
        scheduler
            .reservations
            .retain(|reservation| !canceled.contains(&reservation.entity));
        scheduler.retry_remaining = 0.0;
        let mut controls = queries.p1();
        for &entity in &canceled {
            if let Ok((_, _, _, mut life, _, _, _, _, _, _)) = controls.get_mut(entity) {
                life.respawn_remaining = 0.0;
            }
            commands.entity(entity).remove::<RespawnPlan>();
        }
    }

    // Keep the normal death delay, while allowing a reservation warning to
    // overlap it. A reserved player cannot spawn until both have elapsed.
    {
        let mut controls = queries.p1();
        for &(_, entity, _) in &respawning_scratch {
            let Ok((_, _, _, mut life, _, _, _, _, _, plan)) = controls.get_mut(entity) else {
                continue;
            };
            life.respawn_remaining = (life.respawn_remaining - time.delta_secs()).max(0.0);
            if plan.is_none() {
                // Waiting is represented by no plan. Zero is intentionally
                // retained as the searching state once the death delay ends.
                continue;
            }
        }
    }

    scheduler.retry_remaining = (scheduler.retry_remaining - time.delta_secs()).max(0.0);
    let retry_seconds = if config.respawn_retry_seconds.is_finite() {
        config.respawn_retry_seconds.clamp(0.5, 1.0)
    } else {
        0.5
    };
    if scheduler.retry_remaining <= 0.0 && !scheduler.waiters.is_empty() {
        scheduler.retry_remaining = retry_seconds;
        let batch = config.respawn_candidate_batch.max(1);
        let mut reserved_waiters = Vec::new();
        for index in 0..scheduler.waiters.len() {
            let (entity, mut cursor, stride) = {
                let waiter = scheduler
                    .waiters
                    .get(index)
                    .expect("waiter index remains valid during reservation pass");
                (waiter.entity, waiter.cursor, waiter.stride)
            };
            if scheduler
                .reservations
                .iter()
                .any(|reservation| reservation.entity == entity)
            {
                reserved_waiters.push(entity);
                continue;
            }
            let safety = RespawnSafetyContext {
                board: &board,
                territory: &territory_map,
                living: &living_scratch,
                trails: &trail_scratch,
                reserved: &scheduler.reservations,
                config: &config,
            };
            let Some(position) = choose_respawn(&safety, &mut cursor, stride, batch) else {
                if let Some(waiter) = scheduler.waiters.get_mut(index) {
                    waiter.cursor = cursor;
                }
                continue;
            };
            if let Some(waiter) = scheduler.waiters.get_mut(index) {
                waiter.cursor = cursor;
            }
            scheduler.reservations.push(RespawnReservation {
                entity,
                center: position,
                radius: config.starting_territory_radius,
                neutral_revision: territory_map.revision(),
                neutral: true,
            });
            reserved_waiters.push(entity);
            commands.entity(entity).insert(RespawnPlan {
                position,
                duration: warning_duration(&config),
            });
            if let Ok((_, _, _, mut life, _, _, _, _, _, _)) = queries.p1().get_mut(entity) {
                life.respawn_remaining = life.respawn_remaining.max(warning_duration(&config));
            }
        }
        scheduler
            .waiters
            .retain(|waiter| !reserved_waiters.contains(&waiter.entity));
    }

    let mut controls = queries.p1();
    let reservations = scheduler.reservations.clone();
    let mut spawned = Vec::new();
    for reservation in reservations {
        let Ok((
            entity,
            competitor,
            mut motion,
            mut life,
            mut protection,
            mut territory,
            mut stats,
            mut last_owned,
            trail,
            plan,
        )) = controls.get_mut(reservation.entity)
        else {
            continue;
        };
        if life.respawn_remaining > 0.0 || plan.is_none() {
            continue;
        }
        // Revalidate from current authoritative state immediately before the
        // guarded claim. There is no fallback position or implicit movement.
        let safety = RespawnSafetyContext {
            board: &board,
            territory: &territory_map,
            living: &living_scratch,
            trails: &trail_scratch,
            reserved: &scheduler.reservations,
            config: &config,
        };
        if !safety.is_valid(reservation.center, reservation.radius, Some(entity), None)
            || !claim_respawn_seed(
                &mut territory_map,
                reservation.center,
                reservation.radius,
                competitor.id,
                &mut _displacement_credits,
            )
        {
            scheduler
                .reservations
                .retain(|candidate| candidate.entity != entity);
            scheduler.retry_remaining = 0.0;
            commands.entity(entity).remove::<RespawnPlan>();
            life.respawn_remaining = 0.0;
            continue;
        }
        if trail.is_some() {
            commands.entity(entity).remove::<ActiveTrail>();
        }
        reset_respawn_anchor(&board, &mut motion, &mut last_owned, reservation.center);
        life.status = LifeStatus::Alive;
        life.respawn_remaining = 0.0;
        protection.remaining = config.spawn_protection_seconds;
        protection.elapsed = 0.0;
        territory.current_area = territory_map.area(competitor.id);
        territory.peak_area = territory.peak_area.max(territory.current_area);
        stats.peak_territory_area = stats.peak_territory_area.max(territory.current_area);
        commands.entity(entity).remove::<RespawnPlan>();
        events.0.push(SimulationEvent::Respawn {
            player: competitor.id,
        });
        npc_events.0.push(NpcEventMessage {
            recipient: competitor.id,
            event: NpcEvent::Spawned,
            tick: clock.as_ref().map_or(0, |clock| clock.0),
        });
        spawned.push(entity);
    }
    scheduler
        .reservations
        .retain(|reservation| !spawned.contains(&reservation.entity));
}

struct RespawnSafetyContext<'a> {
    board: &'a BoardGrid,
    territory: &'a TerritoryMap,
    living: &'a [(CompetitorId, Vec2)],
    trails: &'a [(Vec2, Vec2)],
    reserved: &'a [RespawnReservation],
    config: &'a GameConfig,
}

impl RespawnSafetyContext<'_> {
    fn candidate_is_safe(&self, position: Vec2, radius: f32, own_entity: Option<Entity>) -> bool {
        // This center ownership check is a cheap broadphase only. Slivers are
        // rejected by the exact disk boolean after all cheap checks pass.
        if !position.is_finite()
            || self.territory.owner_at(position).competitor().is_some()
            || !self
                .territory
                .arena_boundary()
                .at_least_margin(position, self.config.spawn_boundary_clearance)
            || self
                .living
                .iter()
                .any(|(_, other)| other.distance(position) < self.config.spawn_cube_clearance)
        {
            return false;
        }
        let trail_limit = self.config.spawn_trail_clearance + radius;
        // Board rasterization is conservative and updated by the authoritative
        // trail systems before this set runs. It keeps candidate work cheap; the
        // full segment check is still mandatory for pending-site revalidation.
        if nearest_active_trail_distance(self.board, position, trail_limit) < trail_limit {
            return false;
        }
        // Circle footprints are convex subsets of their radius disks, so this
        // exact center-distance bound guarantees non-overlap without repeating
        // a polygon boolean for every pending plan on every tick.
        self.reserved
            .iter()
            .filter(|r| Some(r.entity) != own_entity)
            .all(|other| {
                other.center.distance(position)
                    >= self.config.spawn_cube_clearance.max(radius + other.radius)
            })
    }

    fn is_valid(
        &self,
        position: Vec2,
        radius: f32,
        own_entity: Option<Entity>,
        neutral_override: Option<bool>,
    ) -> bool {
        if !self.candidate_is_safe(position, radius, own_entity) {
            return false;
        }
        // This is the authoritative trail test, independent of the raster. A
        // stale/missing raster can only delay a respawn, never make it unsafe.
        let trail_limit = self.config.spawn_trail_clearance + radius;
        if self
            .trails
            .iter()
            .any(|&(a, b)| point_segment_distance(position, a, b) < trail_limit)
        {
            return false;
        }
        neutral_override.unwrap_or_else(|| self.territory.is_neutral_seed_site(position, radius))
    }
}

fn choose_respawn(
    safety: &RespawnSafetyContext<'_>,
    cursor: &mut usize,
    stride: usize,
    batch: usize,
) -> Option<Vec2> {
    let count = safety.board.spawn_candidates.len();
    if count == 0 {
        return None;
    }
    // Cursor progress persists on the waiter. A saturated board therefore
    // does bounded work and eventually visits every candidate exactly once
    // per pass. The coprime stride distributes each batch over the whole
    // board instead of spending many retries in one owned row-major region.
    for _ in 0..batch.min(count) {
        let index = safety.board.spawn_candidates[(*cursor * stride) % count];
        *cursor = (*cursor + 1) % count;
        let position = safety.board.cell_center(safety.board.cell(index));
        if !safety.candidate_is_safe(position, safety.config.starting_territory_radius, None) {
            continue;
        }
        // The center test above is only a broadphase. This exact boolean is
        // intentionally reached by a small number of plausible candidates.
        if safety
            .territory
            .is_neutral_seed_site(position, safety.config.starting_territory_radius)
        {
            return Some(position);
        }
    }
    None
}

fn coprime_stride(count: usize, seed: u64) -> usize {
    if count <= 1 {
        return 1;
    }
    let mut stride = (seed as usize % count).max(1);
    while gcd(stride, count) != 1 {
        stride = (stride + 1) % count;
        if stride == 0 {
            stride = 1;
        }
    }
    stride
}

fn gcd(mut left: usize, mut right: usize) -> usize {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

fn warning_duration(config: &GameConfig) -> f32 {
    if config.respawn_warning_seconds.is_finite() {
        config
            .respawn_warning_seconds
            .max(MIN_RESPAWN_WARNING_SECONDS)
    } else {
        MIN_RESPAWN_WARNING_SECONDS
    }
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

pub(super) fn reset_respawn_anchor(
    board: &BoardGrid,
    motion: &mut CompetitorMotion,
    last_owned: &mut LastOwnedCell,
    position: Vec2,
) {
    *motion = CompetitorMotion::new(position, -position);
    last_owned.0 = board
        .world_to_cell(position)
        .expect("respawn candidates are playable board cells");
}

/// Claims only an already validated neutral seed. In particular, this helper
/// cannot displace or silently erase another competitor's territory.
pub(super) fn claim_respawn_seed(
    territory_map: &mut TerritoryMap,
    position: Vec2,
    radius: f32,
    respawning: CompetitorId,
    _credits: &mut DisplacementCredits,
) -> bool {
    if !territory_map.is_neutral_seed_site(position, radius) {
        return false;
    }
    territory_map.seed_owner(position, radius, respawning);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        geometry::MultiPolygon,
        match_game::{
            CompetitorKind, MatchSpec, RosterDescriptor, SimulationPaused, SimulationPlugin,
            start_simulation,
        },
    };
    use std::time::Duration;

    fn scheduler_app(config: GameConfig) -> App {
        let board = BoardGrid::generate(71, 2, &config);
        let territory = TerritoryMap::from_board(&board);
        let mut app = App::new();
        app.insert_resource(config.clone())
            .add_plugins(SimulationPlugin);
        app.update();
        app.world_mut().insert_resource(board);
        app.world_mut().insert_resource(territory);
        app.world_mut().insert_resource(MatchSession {
            field_seed: 71,
            phase: MatchPhase::Running,
            ..MatchSession::default()
        });
        app.world_mut().insert_resource(MatchRules::from(&config));
        let generation = app.world().resource::<super::super::MatchGeneration>().0;
        app.world_mut()
            .resource_mut::<super::super::PresentationReady>()
            .0 = Some(generation);
        app
    }

    fn add_respawning(app: &mut App, id: u8, kind: CompetitorKind) -> Entity {
        let board = app.world().resource::<BoardGrid>();
        let cell = board.cell(board.spawn_candidates[id as usize]);
        let position = board.cell_center(cell);
        app.world_mut()
            .spawn((
                Competitor {
                    id: CompetitorId(id),
                    display_name: format!("{kind:?}"),
                    kind,
                    color_id: id,
                    pattern_id: 0,
                },
                CompetitorMotion::new(position, Vec2::Y),
                LifeState {
                    status: LifeStatus::Respawning,
                    respawn_remaining: 0.0,
                },
                SpawnProtection {
                    remaining: 0.0,
                    elapsed: 0.0,
                },
                TerritoryRecord::default(),
                MatchStatistics::default(),
                LastOwnedCell(cell),
            ))
            .id()
    }

    fn tick(app: &mut App, seconds: f32) {
        app.world_mut()
            .resource_mut::<Time<Fixed>>()
            .advance_by(Duration::from_secs_f32(seconds));
        app.world_mut().run_schedule(FixedUpdate);
    }

    #[test]
    fn scheduler_gives_humans_and_npcs_at_least_five_seconds_warning() {
        let mut app = scheduler_app(GameConfig::default());
        let human = add_respawning(&mut app, 0, CompetitorKind::Human);
        let npc = add_respawning(&mut app, 1, CompetitorKind::Npc);

        tick(&mut app, 0.1);

        for entity in [human, npc] {
            let plan = app.world().entity(entity).get::<RespawnPlan>().copied();
            assert!(plan.is_some_and(|plan| plan.duration >= 5.0));
            assert_eq!(
                app.world()
                    .entity(entity)
                    .get::<LifeState>()
                    .unwrap()
                    .status,
                LifeStatus::Respawning
            );
        }
    }

    #[test]
    fn scheduler_waits_without_a_room_and_does_not_overwrite_territory() {
        let mut app = scheduler_app(GameConfig::default());
        let entity = add_respawning(&mut app, 0, CompetitorKind::Human);
        app.world_mut().resource_mut::<MatchRules>().victory_enabled = false;
        let before = {
            let world = app.world_mut();
            let territory = world.resource::<TerritoryMap>().arena().clone();
            world
                .resource_mut::<TerritoryMap>()
                .apply_claim(CompetitorId(1), territory);
            world.resource::<TerritoryMap>().area(CompetitorId(1))
        };

        tick(&mut app, 0.1);

        assert!(app.world().entity(entity).get::<RespawnPlan>().is_none());
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<LifeState>()
                .unwrap()
                .status,
            LifeStatus::Respawning
        );
        assert_eq!(
            app.world().resource::<TerritoryMap>().area(CompetitorId(1)),
            before
        );
        assert_eq!(app.world().resource::<RespawnScheduler>().waiters.len(), 1);
    }

    #[test]
    fn reservations_keep_spacing_when_the_second_waiter_retries_on_a_later_tick() {
        let config = GameConfig {
            respawn_candidate_batch: 1,
            spawn_cube_clearance: 4.0,
            ..GameConfig::default()
        };
        let mut app = scheduler_app(config.clone());
        let first = add_respawning(&mut app, 0, CompetitorKind::Human);
        let second = add_respawning(&mut app, 1, CompetitorKind::Npc);

        tick(&mut app, 0.1);
        let first_position = app
            .world()
            .entity(first)
            .get::<RespawnPlan>()
            .unwrap()
            .position;
        assert!(app.world().entity(second).get::<RespawnPlan>().is_none());

        for _ in 0..20 {
            tick(&mut app, 0.1);
            if app.world().entity(second).get::<RespawnPlan>().is_some() {
                break;
            }
        }
        let second_position = app
            .world()
            .entity(second)
            .get::<RespawnPlan>()
            .unwrap()
            .position;
        assert!(
            first_position.distance(second_position)
                >= config
                    .spawn_cube_clearance
                    .max(config.starting_territory_radius * 2.0)
        );
    }

    #[test]
    fn capture_at_a_planned_site_cancels_and_restarts_the_warning() {
        let mut app = scheduler_app(GameConfig::default());
        let entity = add_respawning(&mut app, 0, CompetitorKind::Human);

        tick(&mut app, 0.1);
        let old_position = app
            .world()
            .entity(entity)
            .get::<RespawnPlan>()
            .unwrap()
            .position;
        let radius = app
            .world()
            .resource::<GameConfig>()
            .starting_territory_radius;
        let claimed_site = TerritoryMap::seed_footprint(old_position, radius);
        app.world_mut()
            .resource_mut::<TerritoryMap>()
            .apply_claim(CompetitorId(1), claimed_site);

        tick(&mut app, 0.1);
        assert!(app.world().entity(entity).get::<RespawnPlan>().is_none());
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<LifeState>()
                .unwrap()
                .respawn_remaining,
            0.0
        );
        assert!(
            app.world()
                .resource::<RespawnScheduler>()
                .reservations
                .is_empty()
        );

        tick(&mut app, 0.1);
        let new_plan = app.world().entity(entity).get::<RespawnPlan>().unwrap();
        assert_ne!(new_plan.position, old_position);
        assert!(new_plan.duration >= 5.0);
    }

    #[test]
    fn disabled_and_paused_matches_do_not_advance_respawns() {
        let mut app = scheduler_app(GameConfig::default());
        let entity = add_respawning(&mut app, 0, CompetitorKind::Human);
        tick(&mut app, 0.1);
        assert!(app.world().entity(entity).get::<RespawnPlan>().is_some());

        app.world_mut().resource_mut::<MatchRules>().respawn_enabled = false;
        tick(&mut app, 0.1);
        assert_eq!(
            app.world()
                .entity(entity)
                .get::<LifeState>()
                .unwrap()
                .status,
            LifeStatus::Eliminated
        );
        assert!(app.world().entity(entity).get::<RespawnPlan>().is_none());
        assert!(
            app.world()
                .resource::<RespawnScheduler>()
                .reservations
                .is_empty()
        );

        let mut app = scheduler_app(GameConfig::default());
        let entity = add_respawning(&mut app, 0, CompetitorKind::Human);
        app.world_mut().resource_mut::<SimulationPaused>().0 = true;
        tick(&mut app, 0.1);
        assert!(app.world().entity(entity).get::<RespawnPlan>().is_none());
        app.world_mut().resource_mut::<SimulationPaused>().0 = false;
        tick(&mut app, 0.1);
        assert!(app.world().entity(entity).get::<RespawnPlan>().is_some());
    }

    #[test]
    fn victory_cleans_pending_plans_and_reset_clears_scheduler_state() {
        let mut app = scheduler_app(GameConfig::default());
        let human = add_respawning(&mut app, 0, CompetitorKind::Human);
        let npc = add_respawning(&mut app, 1, CompetitorKind::Npc);
        tick(&mut app, 0.1);
        let planned = app
            .world()
            .entity(human)
            .get::<RespawnPlan>()
            .unwrap()
            .position;
        assert!(app.world().entity(npc).get::<RespawnPlan>().is_some());

        app.world_mut()
            .resource_mut::<MatchRules>()
            .victory_threshold_percent = 1;
        let (min, max) = app
            .world()
            .resource::<TerritoryMap>()
            .arena()
            .bounds()
            .unwrap();
        let midpoint = (min.world().x + max.world().x) * 0.5;
        let x = if planned.x > midpoint {
            (min.world().x - 1.0, midpoint - 5.0)
        } else {
            (midpoint + 5.0, max.world().x + 1.0)
        };
        let claim = MultiPolygon::from_outer(&[
            Vec2::new(x.0, min.world().y - 1.0),
            Vec2::new(x.1, min.world().y - 1.0),
            Vec2::new(x.1, max.world().y + 1.0),
            Vec2::new(x.0, max.world().y + 1.0),
        ]);
        app.world_mut()
            .resource_mut::<TerritoryMap>()
            .apply_claim(CompetitorId(0), claim);

        tick(&mut app, 0.1);
        assert_eq!(
            app.world().resource::<MatchSession>().phase,
            MatchPhase::Finished
        );
        assert!(app.world().entity(human).get::<RespawnPlan>().is_none());
        assert!(app.world().entity(npc).get::<RespawnPlan>().is_none());
        assert!(
            app.world()
                .resource::<RespawnScheduler>()
                .reservations
                .is_empty()
        );
        assert!(
            app.world()
                .resource::<RespawnScheduler>()
                .waiters
                .is_empty()
        );
        for entity in [human, npc] {
            assert_eq!(
                app.world()
                    .entity(entity)
                    .get::<LifeState>()
                    .unwrap()
                    .status,
                LifeStatus::Eliminated
            );
        }

        let config = GameConfig::default();
        let spec = MatchSpec::from_config(
            72,
            vec![
                RosterDescriptor::Human {
                    identity: "human".into(),
                    display_name: "Human".into(),
                    color_id: 0,
                    pattern_id: 0,
                },
                RosterDescriptor::Npc,
            ],
            &config,
        );
        start_simulation(app.world_mut(), &spec);
        let scheduler = app.world().resource::<RespawnScheduler>();
        assert!(scheduler.reservations.is_empty());
        assert!(scheduler.waiters.is_empty());
        assert_eq!(scheduler.retry_remaining, 0.0);
        assert!(
            app.world_mut()
                .query::<&RespawnPlan>()
                .iter(app.world())
                .next()
                .is_none()
        );
    }

    #[test]
    fn coprime_candidate_cursor_covers_late_pocket_without_replacement() {
        let count = 97;
        let stride = coprime_stride(count, 0xdead_beef);
        let mut cursor = 0;
        let mut visited = Vec::new();
        for _ in 0..count {
            visited.push((cursor * stride) % count);
            cursor = (cursor + 1) % count;
        }
        visited.sort_unstable();
        visited.dedup();
        assert_eq!(visited, (0..count).collect::<Vec<_>>());
    }

    #[test]
    fn warning_duration_has_the_safety_floor() {
        let config = GameConfig {
            respawn_warning_seconds: 1.0,
            ..GameConfig::default()
        };
        assert_eq!(warning_duration(&config), 5.0);
        let config = GameConfig {
            respawn_warning_seconds: 7.0,
            ..GameConfig::default()
        };
        assert_eq!(warning_duration(&config), 7.0);
    }
}
