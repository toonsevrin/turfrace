use bevy::{prelude::*, time::Fixed};

use crate::{
    board::{BoardGrid, TrailSegmentRef},
    ids::MAX_COMPETITORS,
    match_game::SteeringIntent,
    movement::CompetitorMotion,
    npc::{
        CapturePlanContext, NpcController, NpcVisibleRival, NpcVisibleTrail, build_decision_frame,
        propose_capture_plan, update_colored_error,
    },
    territory_map::{OwnerFrontier, TerritoryMap},
    trail::ActiveTrail,
};

use super::model::{
    Competitor, LifeState, MatchSession, Rankings, SpawnProtection, TerritoryRecord,
};

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

#[allow(clippy::too_many_arguments)]
pub(super) fn npc_think(
    time: Res<Time<Fixed>>,
    board: Res<BoardGrid>,
    territory: Res<TerritoryMap>,
    session: Res<MatchSession>,
    rankings: Res<Rankings>,
    mut queries: ParamSet<(Query<NpcSnapshot>, Query<NpcControl>)>,
    mut people: Local<Vec<NpcPersonSnapshot>>,
    mut trail_perceptions: Local<[Vec<NpcVisibleTrail>; MAX_COMPETITORS]>,
    mut nearby_people: Local<Vec<NpcVisibleRival>>,
    mut nearby_trails: Local<Vec<NpcVisibleTrail>>,
    mut segment_refs: Local<Vec<TrailSegmentRef>>,
    mut frontier_scratch: Local<Vec<OwnerFrontier>>,
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
        queries.p0().iter().map(
            |(competitor, motion, life, territory, trail)| NpcPersonSnapshot {
                id: competitor.id,
                position: motion.position,
                alive: life.is_alive(),
                territory_area: territory.current_area,
                exposed: trail.is_some(),
            },
        ),
    );
    collect_trail_perceptions(
        &board,
        &mut queries,
        &people,
        &mut trail_perceptions,
        &mut segment_refs,
    );

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
        let think_hz = controller.traits.think_hz();
        controller.think_remaining += 1.0 / think_hz;
        let radius = controller.traits.perception_radius();
        nearby_people.clear();
        nearby_people.extend(
            people
                .iter()
                .filter(|person| person.alive && person.id != competitor.id)
                .map(|person| NpcVisibleRival {
                    id: person.id,
                    relative: person.position - motion.position,
                    distance: person.position.distance(motion.position),
                    territory_share: person.territory_area / territory.arena_area().max(1.0),
                    exposed: person.exposed,
                })
                .filter(|rival| rival.distance <= radius),
        );
        sort_visible_rivals(&mut nearby_people);
        nearby_trails.clear();
        nearby_trails.extend(
            trail_perceptions[competitor.id.index()]
                .iter()
                .copied()
                .filter(|trail| trail.distance <= radius),
        );
        let rank = rankings
            .rank_of(competitor.id)
            .map_or(12, |entry| entry.rank);
        controller.prepare_thought(motion.position, 1.0 / think_hz);
        let proposed_capture = if territory.owns(motion.position, competitor.id)
            && !protection.active()
            && controller.memory.active_waypoint().is_none()
        {
            propose_capture_plan(
                CapturePlanContext {
                    board: &board,
                    territory: &territory,
                    id: competitor.id,
                    position: motion.position,
                    heading: motion.heading,
                    visible_rank: rank,
                    traits: controller.traits,
                    memory: &controller.memory,
                    rivals: &nearby_people,
                },
                &mut frontier_scratch,
            )
        } else {
            None
        };
        let frame = build_decision_frame(
            &territory,
            competitor.id,
            motion.position,
            motion.heading,
            protection.active(),
            trail.map_or(0.0, |trail| trail.length),
            rank,
            &nearby_people,
            &nearby_trails,
            controller.traits,
            &controller.memory,
            proposed_capture,
        );
        controller.decide(&frame);
        update_colored_error(&mut controller, competitor.id, session.elapsed_seconds);
        controller.write_steering(&mut steering);
    }
}

fn collect_trail_perceptions(
    board: &BoardGrid,
    queries: &mut ParamSet<(Query<NpcSnapshot>, Query<NpcControl>)>,
    people: &[NpcPersonSnapshot],
    trail_perceptions: &mut [Vec<NpcVisibleTrail>; MAX_COMPETITORS],
    segment_refs: &mut Vec<TrailSegmentRef>,
) {
    trail_perceptions
        .iter_mut()
        .for_each(|perceptions| perceptions.clear());
    for viewer in people {
        board.collect_nearby_trail_segments(viewer.position, 28.0, segment_refs);
        let snapshots = queries.p0();
        for reference in segment_refs.iter().copied() {
            let Some((_, _, _, _, trail)) = snapshots
                .iter()
                .find(|(owner, _, _, _, _)| owner.id == reference.owner)
            else {
                continue;
            };
            let Some((a, b)) = trail.and_then(|trail| trail.segment(reference.segment)) else {
                continue;
            };
            let nearest_point = nearest_point_on_segment(viewer.position, a, b);
            let perception = NpcVisibleTrail {
                owner: reference.owner,
                relative: nearest_point - viewer.position,
                distance: nearest_point.distance(viewer.position),
                own: reference.owner == viewer.id,
            };
            let entries = &mut trail_perceptions[viewer.id.index()];
            if let Some(existing) = entries
                .iter_mut()
                .find(|trail| trail.owner == reference.owner)
            {
                if perception.distance < existing.distance {
                    *existing = perception;
                }
            } else {
                entries.push(perception);
            }
        }
        sort_visible_trails(&mut trail_perceptions[viewer.id.index()]);
    }
}

#[derive(Clone, Copy)]
pub(super) struct NpcPersonSnapshot {
    id: crate::ids::CompetitorId,
    position: Vec2,
    alive: bool,
    territory_area: f32,
    exposed: bool,
}

fn sort_visible_rivals(rivals: &mut [NpcVisibleRival]) {
    rivals.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then_with(|| a.id.cmp(&b.id))
    });
}

fn sort_visible_trails(trails: &mut [NpcVisibleTrail]) {
    trails.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then_with(|| a.owner.cmp(&b.owner))
    });
}

fn nearest_point_on_segment(point: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let segment = b - a;
    if segment.length_squared() <= 1e-8 {
        return a;
    }
    a + segment * ((point - a).dot(segment) / segment.length_squared()).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::CompetitorId;

    #[test]
    fn perception_ties_are_ordered_by_competitor_id() {
        let mut rivals = vec![
            NpcVisibleRival {
                id: CompetitorId(3),
                distance: 4.0,
                ..default()
            },
            NpcVisibleRival {
                id: CompetitorId(1),
                distance: 4.0,
                ..default()
            },
        ];
        sort_visible_rivals(&mut rivals);
        assert_eq!(
            rivals.iter().map(|rival| rival.id).collect::<Vec<_>>(),
            vec![CompetitorId(1), CompetitorId(3)]
        );

        let mut trails = vec![
            NpcVisibleTrail {
                owner: CompetitorId(2),
                distance: 4.0,
                ..default()
            },
            NpcVisibleTrail {
                owner: CompetitorId(0),
                distance: 4.0,
                ..default()
            },
        ];
        sort_visible_trails(&mut trails);
        assert_eq!(
            trails.iter().map(|trail| trail.owner).collect::<Vec<_>>(),
            vec![CompetitorId(0), CompetitorId(2)]
        );
    }
}
