use super::model::{
    Competitor, LastOwnedCell, LifeState, MatchStatistics, Rankings, SpawnProtection,
    TerritoryRecord,
};
use crate::{
    board::{BoardGrid, TrailSegmentRef},
    config::GameConfig,
    ids::MAX_COMPETITORS,
    match_game::SteeringIntent,
    movement::CompetitorMotion,
    npc::{
        CaptureScratch, NpcController, NpcTickContext, NpcTrailSnapshot, NpcVisibleRival,
        build_observation, collect_bounded_nearby_trail_segments,
    },
    territory_map::TerritoryMap,
    trail::ActiveTrail,
};
use bevy::{prelude::*, time::Fixed};

type NpcSnapshot = (
    &'static Competitor,
    &'static CompetitorMotion,
    &'static LifeState,
    &'static TerritoryRecord,
    &'static MatchStatistics,
    Option<&'static ActiveTrail>,
);
type NpcControl = (
    &'static Competitor,
    &'static CompetitorMotion,
    &'static LifeState,
    &'static SpawnProtection,
    &'static MatchStatistics,
    &'static LastOwnedCell,
    Option<&'static ActiveTrail>,
    &'static mut NpcController,
    &'static mut SteeringIntent,
);

#[derive(Clone, Copy)]
pub(super) struct BodySnapshot {
    id: crate::ids::CompetitorId,
    position: Vec2,
    heading: Vec2,
    speed: f32,
    alive: bool,
    area: f32,
    exposed: bool,
}

const NPC_SNAPSHOT_TRAIL_SEGMENT_CAP: usize =
    crate::npc::NPC_RELEVANT_TRAIL_SEGMENT_CAP + MAX_COMPETITORS;

fn retain_snapshot_trail_reference(output: &mut Vec<TrailSegmentRef>, reference: TrailSegmentRef) {
    if reference.owner.index() < MAX_COMPETITORS
        && !output.contains(&reference)
        && output.len() < NPC_SNAPSHOT_TRAIL_SEGMENT_CAP
    {
        output.push(reference);
    }
}

#[derive(Default)]
pub(super) struct NpcWorldSnapshot {
    people: Vec<BodySnapshot>,
    trails: [Option<NpcTrailSnapshot>; MAX_COMPETITORS],
    /// Union of bounded spatial selections around the NPCs. Each owner has a
    /// fixed budget large enough for every NPC's fair selection; keeping this
    /// outside each observation avoids cloning a whole trail per NPC.
    relevant_refs: [Vec<crate::board::TrailSegmentRef>; MAX_COMPETITORS],
    nearby_refs: Vec<crate::board::TrailSegmentRef>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn npc_think(
    time: Res<Time<Fixed>>,
    config: Res<GameConfig>,
    board: Res<BoardGrid>,
    territory: Res<TerritoryMap>,
    clock: Res<super::model::SimulationClock>,
    rankings: Res<Rankings>,
    mut queries: ParamSet<(Query<NpcSnapshot>, Query<NpcControl>)>,
    mut world: Local<NpcWorldSnapshot>,
    mut scratch: Local<CaptureScratch>,
) {
    let dt = time.delta_secs();
    let should_think = queries
        .p1()
        .iter()
        .any(|(_, _, life, _, _, _, _, controller, _)| {
            life.is_alive() && controller.think_remaining <= dt
        });
    if !should_think {
        for (_, _, life, _, _, _, _, mut controller, _) in queries.p1().iter_mut() {
            if life.is_alive() {
                controller.think_remaining -= dt;
            }
        }
        return;
    }
    let world = &mut *world;
    world.people.clear();
    world.trails.iter_mut().for_each(|x| *x = None);
    world.relevant_refs.iter_mut().for_each(|refs| refs.clear());
    for (competitor, motion, life, territory_record, stats, trail) in queries.p0().iter() {
        world.people.push(BodySnapshot {
            id: competitor.id,
            position: motion.position,
            heading: motion.heading,
            speed: config.player_speed_for_kills(stats.kills),
            alive: life.is_alive(),
            area: territory_record.current_area,
            exposed: trail.is_some(),
        });
        if let Some(trail) = trail {
            let segment_start = trail
                .segment_count()
                .saturating_sub(crate::npc::NPC_TRAIL_SNAPSHOT_CAP);
            world.trails[competitor.id.index()] = Some(NpcTrailSnapshot {
                owner: competitor.id,
                segment_start,
                spatial_segments: Vec::new(),
                segments: (segment_start..trail.segment_count())
                    .filter_map(|i| trail.segment(i))
                    .collect(),
            });
        }
    }
    // Select indexed history around NPCs before constructing observations.
    // This is a shared union, rather than one cloned trail per NPC.
    // The per-observation query is capped fairly by owner. The shared sparse
    // snapshot has a slightly larger fixed per-owner budget so a segment
    // selected for one NPC cannot be evicted by another NPC's selection.
    {
        for (_, motion, life, _, _, _, _, controller, _) in queries.p1().iter_mut() {
            if !life.is_alive() {
                continue;
            }
            collect_bounded_nearby_trail_segments(
                &board,
                motion.position,
                controller.profile.competence.sensor_horizon(),
                &mut world.nearby_refs,
            );
            for reference in world.nearby_refs.iter().copied() {
                retain_snapshot_trail_reference(
                    &mut world.relevant_refs[reference.owner.index()],
                    reference,
                );
            }
        }
    }
    // Resolve only the selected sparse refs while the authoritative ECS trail
    // is borrowed. Older indexed segments are now available alongside the
    // newest contiguous snapshot tail.
    for (competitor, _, _, _, _, trail) in queries.p0().iter() {
        let Some(trail) = trail else {
            continue;
        };
        let index = competitor.id.index();
        let Some(snapshot) = world.trails[index].as_mut() else {
            continue;
        };
        let segment_start = snapshot.segment_start;
        for reference in world.relevant_refs[index].iter().copied() {
            if reference.segment >= segment_start {
                continue;
            }
            if let Some((start, end)) = trail.segment(reference.segment) {
                snapshot
                    .spatial_segments
                    .push((reference.segment, start, end));
            }
        }
    }
    for (
        competitor,
        motion,
        life,
        protection,
        stats,
        last_owned,
        trail,
        mut controller,
        mut steering,
    ) in queries.p1().iter_mut()
    {
        if !life.is_alive() {
            continue;
        }
        controller.think_remaining -= dt;
        if controller.think_remaining > 0.0 {
            continue;
        }
        let hz = controller.profile.competence.think_hz();
        controller.think_remaining += 1.0 / hz;
        let rivals: Vec<_> = world
            .people
            .iter()
            .filter(|p| p.alive && p.id != competitor.id)
            .map(|p| NpcVisibleRival {
                id: p.id,
                relative: p.position - motion.position,
                distance: p.position.distance(motion.position),
                territory_share: p.area / territory.arena_area().max(1.0),
                exposed: p.exposed,
                heading: p.heading,
                speed: p.speed,
            })
            .collect();
        let rank = rankings
            .rank_of(competitor.id)
            .map_or(12, |entry| entry.rank);
        let speed = config.player_speed_for_kills(stats.kills);
        let observation = build_observation(
            &board,
            &territory,
            competitor.id,
            motion.position,
            motion.heading,
            speed,
            protection.active(),
            trail.map_or(0.0, |t| t.length),
            &rivals,
            &world.trails,
            controller.profile,
            &controller.memory,
        );
        let context = NpcTickContext {
            board: &board,
            territory: &territory,
            config: &config,
            rank,
            tick: clock.0,
            speed,
            last_owned: last_owned.0,
            own_trail: trail,
        };
        controller.tick(&observation, *motion, &context, &mut scratch);
        controller.write_steering(&mut steering);
    }
}
