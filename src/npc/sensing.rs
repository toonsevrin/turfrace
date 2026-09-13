use super::*;
use crate::{
    board::{BoardGrid, TrailSegmentRef},
    ids::{CompetitorId, MAX_COMPETITORS},
    territory_map::TerritoryMap,
};
use bevy::prelude::*;

#[derive(Clone, Debug)]
pub struct NpcTrailSnapshot {
    pub owner: CompetitorId,
    /// The contiguous tail retained for every owner.
    pub segment_start: usize,
    pub segments: Vec<(Vec2, Vec2)>,
    /// Sparse segments selected from the board's spatial index. These are
    /// separate from the contiguous tail because an old segment cannot be
    /// represented by shifting the tail's start.
    pub spatial_segments: Vec<(usize, Vec2, Vec2)>,
}

/// Keep spatial-index collection bounded while giving every owner a fair
/// share. The lower segment numbers are retained when a bucket contains more
/// history than the share; this is deterministic and favors older indexed
/// history over a long remote tail. The exact current head is inspected separately.
#[cfg(test)]
fn retain_trail_reference(output: &mut Vec<TrailSegmentRef>, reference: TrailSegmentRef) {
    if reference.owner.index() >= MAX_COMPETITORS || output.contains(&reference) {
        return;
    }
    let base = NPC_RELEVANT_TRAIL_SEGMENT_CAP / MAX_COMPETITORS;
    let remainder = NPC_RELEVANT_TRAIL_SEGMENT_CAP % MAX_COMPETITORS;
    let limit = base + usize::from(reference.owner.index() < remainder);
    let owner_count = output
        .iter()
        .filter(|existing| existing.owner == reference.owner)
        .count();
    if owner_count < limit {
        output.push(reference);
        return;
    }
    let Some((farthest, _)) = output
        .iter()
        .enumerate()
        .filter(|(_, existing)| existing.owner == reference.owner)
        .max_by_key(|(_, existing)| existing.segment)
    else {
        return;
    };
    if output[farthest].segment > reference.segment {
        output[farthest] = reference;
    }
}

/// Query buckets without allowing a large bucket to grow a temporary vector.
/// The canonical board visitor lets us apply the per-owner cap before allocating.
pub(crate) fn collect_bounded_nearby_trail_segments(
    board: &BoardGrid,
    position: Vec2,
    radius: f32,
    output: &mut Vec<TrailSegmentRef>,
) {
    // Bucket references repeat across neighboring cells. Keep each owner's
    // tiny sorted share separately instead of scanning the whole output up to
    // three times for every visited reference. Storage remains strictly bounded.
    const OWNER_SLOTS: usize = NPC_RELEVANT_TRAIL_SEGMENT_CAP.div_ceil(MAX_COMPETITORS);
    let mut segments = [[0usize; OWNER_SLOTS]; MAX_COMPETITORS];
    let mut counts = [0usize; MAX_COMPETITORS];
    board.visit_nearby_trail_segments(position, radius, |reference| {
        let owner = reference.owner.index();
        if owner >= MAX_COMPETITORS {
            return;
        }
        let limit = NPC_RELEVANT_TRAIL_SEGMENT_CAP / MAX_COMPETITORS
            + usize::from(owner < NPC_RELEVANT_TRAIL_SEGMENT_CAP % MAX_COMPETITORS);
        let count = counts[owner];
        if limit == 0 || (count == limit && reference.segment >= segments[owner][count - 1]) {
            return;
        }
        let Err(slot) = segments[owner][..count].binary_search(&reference.segment) else {
            return;
        };
        let retained = count.min(limit - 1);
        segments[owner].copy_within(slot..retained, slot + 1);
        segments[owner][slot] = reference.segment;
        counts[owner] = (count + 1).min(limit);
    });
    output.clear();
    for owner in 0..MAX_COMPETITORS {
        output.extend(
            segments[owner][..counts[owner]]
                .iter()
                .map(|&segment| TrailSegmentRef {
                    owner: CompetitorId(owner as u8),
                    segment,
                }),
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_observation(
    board: &BoardGrid,
    territory: &TerritoryMap,
    self_id: CompetitorId,
    position: Vec2,
    heading: Vec2,
    speed: f32,
    protected: bool,
    trail_length: f32,
    rivals: &[NpcVisibleRival],
    trails: &[Option<NpcTrailSnapshot>; MAX_COMPETITORS],
    profile: NpcProfile,
    memory: &NpcEventMemory,
) -> NpcObservation {
    let radius = profile.competence.sensor_horizon();
    let mut sorted_rivals: Vec<_> = rivals
        .iter()
        .copied()
        .filter(|r| r.id != self_id && r.distance <= radius)
        .collect();
    sorted_rivals.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut visible_rivals = [None; NPC_VISIBLE_RIVAL_CAP];
    for (slot, rival) in sorted_rivals
        .into_iter()
        .take(NPC_VISIBLE_RIVAL_CAP)
        .enumerate()
    {
        visible_rivals[slot] = Some(rival);
    }

    // Bucket collection is bounded per owner. In particular, do not call the
    // board convenience query here: it materializes every historical ref in
    // the nearby cells before a caller can apply a cap.
    let mut refs = Vec::<TrailSegmentRef>::with_capacity(NPC_RELEVANT_TRAIL_SEGMENT_CAP);
    collect_bounded_nearby_trail_segments(board, position, radius, &mut refs);
    let indexed_refs_empty = refs.is_empty();
    let mut segments = Vec::<NpcVisibleSegment>::with_capacity(
        NPC_RELEVANT_TRAIL_SEGMENT_CAP + MAX_COMPETITORS * 2,
    );
    for reference in refs {
        let Some(snapshot) = trails.get(reference.owner.index()).and_then(|x| x.as_ref()) else {
            continue;
        };
        let Some((a, b)) = snapshot_segment(snapshot, reference.segment) else {
            continue;
        };
        append_visible_segment(
            &mut segments,
            self_id,
            position,
            radius,
            reference.owner,
            reference.segment,
            a,
            b,
        );
    }
    // Rasterization is incremental, so the final movement step may not have a
    // bucket reference yet. Always inspect every owner's exact head, not only
    // when the global reference list happens to be empty.
    for snapshot in trails.iter().flatten() {
        let Some(index) = snapshot.segments.len().checked_sub(1) else {
            continue;
        };
        let segment = snapshot.segment_start + index;
        let Some((a, b)) = snapshot.segments.get(index).copied() else {
            continue;
        };
        append_visible_segment(
            &mut segments,
            self_id,
            position,
            radius,
            snapshot.owner,
            segment,
            a,
            b,
        );
    }
    // Pure/unit-test snapshots may not have been rasterized at all. In that
    // case the bounded snapshot is the index. The snapshot is capped at 128
    // contiguous segments per owner, so this fallback has a fixed upper bound.
    if indexed_refs_empty {
        for snapshot in trails.iter().flatten() {
            for (index, &(a, b)) in snapshot.segments.iter().enumerate() {
                append_visible_segment(
                    &mut segments,
                    self_id,
                    position,
                    radius,
                    snapshot.owner,
                    snapshot.segment_start + index,
                    a,
                    b,
                );
            }
            for &(segment, a, b) in &snapshot.spatial_segments {
                append_visible_segment(
                    &mut segments,
                    self_id,
                    position,
                    radius,
                    snapshot.owner,
                    segment,
                    a,
                    b,
                );
            }
        }
    }
    segments.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then_with(|| a.owner.cmp(&b.owner))
            .then_with(|| a.segment.cmp(&b.segment))
    });
    // Preserve one nearest relevant segment for every visible owner before
    // filling the remaining slots by distance. Crowded trails cannot starve a
    // rival owner out of the encounter facts.
    let mut retained = Vec::with_capacity(NPC_VISIBLE_SEGMENT_CAP);
    for owner in 0..MAX_COMPETITORS {
        if let Some(segment) = segments
            .iter()
            .find(|s| s.owner == CompetitorId(owner as u8))
        {
            retained.push(*segment);
        }
    }
    for segment in segments {
        if retained.len() >= NPC_VISIBLE_SEGMENT_CAP {
            break;
        }
        if !retained
            .iter()
            .any(|s: &NpcVisibleSegment| s.owner == segment.owner && s.segment == segment.segment)
        {
            retained.push(segment);
        }
    }
    retained.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then_with(|| a.owner.cmp(&b.owner))
            .then_with(|| a.segment.cmp(&b.segment))
    });
    let mut visible_segments = [None; NPC_VISIBLE_SEGMENT_CAP];
    for (slot, segment) in retained.into_iter().enumerate() {
        visible_segments[slot] = Some(segment);
    }
    let encounter = encounter(
        position,
        &visible_segments,
        &visible_rivals,
        memory,
        radius,
        speed,
    );
    let heading = heading.normalize_or(Vec2::Y);
    NpcObservation {
        position,
        heading,
        speed,
        protected,
        owns_current_cell: territory.owns(position, self_id),
        trail_length,
        edge_distance: territory.arena_signed_distance(position),
        inward_direction: territory.arena_inward_normal(position),
        home: territory.nearest_owner_frontier(position, self_id, radius * 1.6),
        rivals: visible_rivals,
        segments: visible_segments,
        encounter,
    }
}
fn snapshot_segment(snapshot: &NpcTrailSnapshot, segment: usize) -> Option<(Vec2, Vec2)> {
    snapshot
        .spatial_segments
        .iter()
        .find(|(indexed, _, _)| *indexed == segment)
        .map(|(_, start, end)| (*start, *end))
        .or_else(|| {
            segment
                .checked_sub(snapshot.segment_start)
                .and_then(|index| snapshot.segments.get(index).copied())
        })
}

#[allow(clippy::too_many_arguments)]
fn append_visible_segment(
    segments: &mut Vec<NpcVisibleSegment>,
    self_id: CompetitorId,
    position: Vec2,
    radius: f32,
    owner: CompetitorId,
    segment: usize,
    start: Vec2,
    end: Vec2,
) {
    if segments
        .iter()
        .any(|visible| visible.owner == owner && visible.segment == segment)
    {
        return;
    }
    let nearest = nearest_point_on_segment(position, start, end);
    let distance = nearest.distance(position);
    if distance <= radius {
        segments.push(NpcVisibleSegment {
            owner,
            segment,
            start,
            end,
            nearest_point: nearest,
            relative: nearest - position,
            distance,
            tangent: (end - start).normalize_or(Vec2::X),
            own: owner == self_id,
        });
    }
}

fn encounter(
    observer_position: Vec2,
    segments: &[Option<NpcVisibleSegment>; NPC_VISIBLE_SEGMENT_CAP],
    rivals: &[Option<NpcVisibleRival>; NPC_VISIBLE_RIVAL_CAP],
    memory: &NpcEventMemory,
    radius: f32,
    speed: f32,
) -> NpcEncounter {
    if let Some(segment) = segments.iter().flatten().find(|s| !s.own) {
        // A trail is static evidence, but the owner body (when visible) is
        // still the only valid source for a return ETA. Never infer body
        // velocity from the trail tangent.
        let rival = rivals
            .iter()
            .flatten()
            .find(|rival| rival.id == segment.owner);
        let velocity = rival.map_or(segment.tangent * speed, |rival| rival.heading * rival.speed);
        return NpcEncounter {
            target: Some(HuntTarget::Segment {
                owner: segment.owner,
                segment: segment.segment,
            }),
            position: segment.nearest_point,
            velocity,
            distance: segment.distance,
            intercept_time: segment.distance / velocity.length().max(1.0),
            confidence: (1.0 - segment.distance / radius).clamp(0.0, 1.0),
        };
    }
    if let Some(rival) = rivals.iter().flatten().find(|r| r.exposed) {
        let known = memory.opponents[rival.id.index()].observations > 0;
        return NpcEncounter {
            target: Some(HuntTarget::Rival(rival.id)),
            position: observer_position + rival.relative,
            velocity: rival.heading * rival.speed,
            distance: rival.distance,
            intercept_time: rival.distance / rival.speed.max(1.0),
            confidence: (1.0 - rival.distance / radius).clamp(0.0, 1.0)
                * (if known { 0.9 } else { 0.7 }),
        };
    }
    NpcEncounter::default()
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
    use crate::{board::BoardGrid, config::GameConfig, territory_map::TerritoryMap};
    fn profile(a: f32) -> NpcProfile {
        NpcProfile {
            policy: NpcPolicy::Builder(BuilderPolicy {
                shape: BuilderShape::Fill,
                side: TurnSide::Left,
            }),
            competence: NpcCompetence { skill: a },
        }
    }
    #[test]
    fn same_radius_cap_and_stable_ties() {
        let cfg = GameConfig::default();
        let b = BoardGrid::generate(9, 2, &cfg);
        let t = TerritoryMap::from_board(&b);
        let mut trails: [Option<NpcTrailSnapshot>; MAX_COMPETITORS] = std::array::from_fn(|_| None);
        trails[1] = Some(NpcTrailSnapshot {
            owner: CompetitorId(1),
            segment_start: 0,
            segments: (0..30)
                .map(|i| {
                    (
                        Vec2::new(1.0 + i as f32, 0.0),
                        Vec2::new(1.1 + i as f32, 0.0),
                    )
                })
                .collect(),
            spatial_segments: Vec::new(),
        });
        let rivals = [NpcVisibleRival {
            id: CompetitorId(1),
            relative: Vec2::X * 15.0,
            distance: 15.0,
            ..default()
        }];
        let o = build_observation(
            &b,
            &t,
            CompetitorId(0),
            Vec2::ZERO,
            Vec2::Y,
            8.0,
            false,
            0.0,
            &rivals,
            &trails,
            profile(0.0),
            &NpcEventMemory::new(),
        );
        assert!(o.segments.iter().flatten().count() <= NPC_VISIBLE_SEGMENT_CAP);
        assert!(o.rivals.iter().flatten().all(|r| r.distance <= 16.0));
    }
    #[test]
    fn multiple_segments_from_owner_survive() {
        let b = BoardGrid::generate(10, 2, &GameConfig::default());
        let t = TerritoryMap::from_board(&b);
        let mut trails: [Option<NpcTrailSnapshot>; MAX_COMPETITORS] = std::array::from_fn(|_| None);
        trails[1] = Some(NpcTrailSnapshot {
            owner: CompetitorId(1),
            segment_start: 0,
            segments: vec![
                (Vec2::new(-2.0, 0.0), Vec2::new(-1.0, 0.0)),
                (Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)),
            ],
            spatial_segments: Vec::new(),
        });
        let o = build_observation(
            &b,
            &t,
            CompetitorId(0),
            Vec2::ZERO,
            Vec2::Y,
            8.0,
            false,
            0.0,
            &[],
            &trails,
            profile(1.0),
            &NpcEventMemory::new(),
        );
        assert!(
            o.segments
                .iter()
                .flatten()
                .filter(|s| s.owner == CompetitorId(1))
                .count()
                >= 2
        );
    }

    #[test]
    fn old_indexed_segment_survives_a_long_remote_tail() {
        let config = GameConfig::default();
        let mut board = BoardGrid::generate(41, 2, &config);
        let cell = board.world_to_cell(Vec2::ZERO).unwrap();
        let index = board.index(cell).unwrap();
        board.trail_segment_buckets[index].push(TrailSegmentRef {
            owner: CompetitorId(1),
            segment: 3,
        });
        let territory = TerritoryMap::from_board(&board);
        let mut trails: [Option<NpcTrailSnapshot>; MAX_COMPETITORS] = std::array::from_fn(|_| None);
        trails[1] = Some(NpcTrailSnapshot {
            owner: CompetitorId(1),
            segment_start: 128,
            segments: (128..256)
                .map(|i| {
                    (
                        Vec2::new(100.0 + i as f32, 100.0),
                        Vec2::new(100.1 + i as f32, 100.0),
                    )
                })
                .collect(),
            spatial_segments: vec![(3, Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0))],
        });
        let observation = build_observation(
            &board,
            &territory,
            CompetitorId(0),
            Vec2::ZERO,
            Vec2::Y,
            8.0,
            false,
            0.0,
            &[],
            &trails,
            profile(0.0),
            &NpcEventMemory::new(),
        );
        assert!(
            observation
                .segments
                .iter()
                .flatten()
                .any(|segment| { segment.owner == CompetitorId(1) && segment.segment == 3 })
        );
    }

    #[test]
    fn partitioned_collection_matches_reference_with_duplicates_and_permutations() {
        let config = GameConfig::default();
        let mut board = BoardGrid::generate(42, 2, &config);
        let cell = board.world_to_cell(Vec2::ZERO).unwrap();
        let index = board.index(cell).unwrap();
        for order in 0..5 {
            board.trail_segment_buckets[index].clear();
            for step in 0..4096 {
                board.trail_segment_buckets[index].push(TrailSegmentRef {
                    owner: CompetitorId(((step * 7 + order) % (MAX_COMPETITORS + 2)) as u8),
                    segment: ((4096 - step) * (order * 2 + 1)) % 127,
                });
            }
            let mut expected = Vec::new();
            board.visit_nearby_trail_segments(Vec2::ZERO, 16.0, |reference| {
                retain_trail_reference(&mut expected, reference)
            });
            expected.sort_unstable_by_key(|reference| (reference.owner, reference.segment));
            let mut actual = vec![TrailSegmentRef {
                owner: CompetitorId(0),
                segment: usize::MAX,
            }];
            collect_bounded_nearby_trail_segments(&board, Vec2::ZERO, 16.0, &mut actual);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn spatial_collection_is_bounded_and_fair_per_owner() {
        let config = GameConfig::default();
        let mut board = BoardGrid::generate(42, 2, &config);
        let cell = board.world_to_cell(Vec2::ZERO).unwrap();
        let index = board.index(cell).unwrap();
        for segment in 0..100 {
            board.trail_segment_buckets[index].push(TrailSegmentRef {
                owner: CompetitorId(1),
                segment,
            });
        }
        for owner in [CompetitorId(2), CompetitorId(3)] {
            board.trail_segment_buckets[index].push(TrailSegmentRef { owner, segment: 7 });
        }
        let mut refs = Vec::new();
        collect_bounded_nearby_trail_segments(&board, Vec2::ZERO, 16.0, &mut refs);
        assert!(refs.len() <= NPC_RELEVANT_TRAIL_SEGMENT_CAP);
        for owner in [CompetitorId(1), CompetitorId(2), CompetitorId(3)] {
            let count = refs
                .iter()
                .filter(|reference| reference.owner == owner)
                .count();
            let limit = NPC_RELEVANT_TRAIL_SEGMENT_CAP / MAX_COMPETITORS
                + usize::from(owner.index() < NPC_RELEVANT_TRAIL_SEGMENT_CAP % MAX_COMPETITORS);
            assert!(count <= limit);
        }
        assert!(
            refs.iter()
                .any(|reference| { reference.owner == CompetitorId(1) && reference.segment == 0 })
        );
        assert!(
            refs.iter()
                .any(|reference| reference.owner == CompetitorId(2))
        );
        assert!(
            refs.iter()
                .any(|reference| reference.owner == CompetitorId(3))
        );
    }
}
