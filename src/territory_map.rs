//! Authoritative vector territory state.
//!
//! The sample grid is intentionally absent from this module.  It is only
//! rebuilt after a committed geometry change for broadphase/render helpers;
//! containment, capture, scoring and elimination all use this map directly.

use bevy::prelude::*;

use crate::{
    board::BoardGrid,
    geometry::{MultiPolygon, Point},
    ids::{CompetitorId, MAX_COMPETITORS, OwnerId},
    trail::ActiveTrail,
};

const CIRCLE_SAMPLES: usize = 32;
const STROKE_SAMPLES: usize = 12;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorCaptureResult {
    pub claim: MultiPolygon,
    pub claimed_area: f32,
    pub stolen_by_owner: Vec<(CompetitorId, f32)>,
    pub used_loop_fill: bool,
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct TerritoryMap {
    pub arena: MultiPolygon,
    pub territories: [MultiPolygon; MAX_COMPETITORS],
    pub arena_area: f32,
    pub revision: u64,
    index: TerritorySpatialIndex,
}

impl Default for TerritoryMap {
    fn default() -> Self {
        Self::new(MultiPolygon::empty())
    }
}

impl TerritoryMap {
    pub fn new(arena: MultiPolygon) -> Self {
        let arena_area = arena.area();
        Self {
            arena,
            territories: std::array::from_fn(|_| MultiPolygon::empty()),
            arena_area,
            revision: 1,
            index: TerritorySpatialIndex::default(),
        }
    }

    pub fn from_board(board: &BoardGrid) -> Self {
        Self::new(MultiPolygon::from_outer(&board.contour.points))
    }

    pub fn area(&self, player: CompetitorId) -> f32 {
        self.territories[player.index()].area()
    }

    pub fn area_percent(&self, player: CompetitorId) -> f32 {
        if self.arena_area <= f32::EPSILON {
            0.0
        } else {
            self.area(player) * 100.0 / self.arena_area
        }
    }

    /// Returns whether a player owns at least the requested share of the
    /// arena. Both areas are fixed-point integer values, so the threshold is
    /// independent of floating-point display rounding and sample-grid size.
    pub fn reaches_victory_threshold(&self, player: CompetitorId, percent: u8) -> bool {
        let arena_area = self.arena.area_scaled();
        if arena_area <= 0 || percent == 0 {
            return false;
        }
        i128::from(self.territories[player.index()].area_scaled()) * 100
            >= i128::from(arena_area) * i128::from(percent)
    }

    pub fn owner_at(&self, point: Vec2) -> OwnerId {
        let candidates = self.index.candidates(point);
        if candidates.is_empty() {
            for (index, territory) in self.territories.iter().enumerate() {
                if territory.contains_world(point) {
                    return CompetitorId(index as u8).owner();
                }
            }
        } else {
            for &index in candidates {
                if self.territories[index as usize].contains_world(point) {
                    return CompetitorId(index).owner();
                }
            }
        }
        OwnerId::UNCLAIMED
    }

    pub fn owns(&self, point: Vec2, player: CompetitorId) -> bool {
        let candidates = self.index.candidates(point);
        (candidates.is_empty() || candidates.contains(&(player.index() as u8)))
            && self.territories[player.index()].contains_world(point)
    }

    pub fn boundary_crossing(&self, player: CompetitorId, from: Vec2, to: Vec2) -> Vec2 {
        let mut owned = 0.0;
        let mut unowned = 1.0;
        for _ in 0..14 {
            let mid = (owned + unowned) * 0.5;
            if self.owns(from.lerp(to, mid), player) {
                owned = mid;
            } else {
                unowned = mid;
            }
        }
        from.lerp(to, unowned)
    }

    pub fn boundary_entry_time(&self, player: CompetitorId, from: Vec2, to: Vec2) -> Option<f32> {
        if self.owns(from, player) {
            return Some(0.0);
        }
        if !self.owns(to, player) {
            return None;
        }
        let mut unowned = 0.0;
        let mut owned = 1.0;
        for _ in 0..18 {
            let mid = (unowned + owned) * 0.5;
            if self.owns(from.lerp(to, mid), player) {
                owned = mid;
            } else {
                unowned = mid;
            }
        }
        Some(owned)
    }

    pub fn arena_signed_distance(&self, point: Vec2) -> f32 {
        if self.arena.is_empty() {
            -f32::INFINITY
        } else {
            let distance = self.arena.boundary_distance(point);
            if self.arena.contains_world(point) {
                distance
            } else {
                -distance
            }
        }
    }

    pub fn arena_inward_normal(&self, point: Vec2) -> Vec2 {
        let epsilon = 0.25;
        let dx = self.arena_signed_distance(point + Vec2::X * epsilon)
            - self.arena_signed_distance(point - Vec2::X * epsilon);
        let dy = self.arena_signed_distance(point + Vec2::Y * epsilon)
            - self.arena_signed_distance(point - Vec2::Y * epsilon);
        Vec2::new(dx, dy)
            .try_normalize()
            .unwrap_or_else(|| -point.try_normalize().unwrap_or(Vec2::Y))
    }

    pub fn nearest_arena_interior(&self, point: Vec2, margin: f32) -> Vec2 {
        if self.arena_signed_distance(point) >= margin {
            return point;
        }
        let normal = self.arena_inward_normal(point);
        let mut candidate = point;
        for _ in 0..12 {
            candidate += normal * margin.max(0.25);
            if self.arena_signed_distance(candidate) >= margin {
                return candidate;
            }
        }
        Vec2::ZERO
    }

    pub fn claim_disk(&mut self, center: Vec2, radius: f32, player: CompetitorId) -> f32 {
        let disk = circle(center, radius, CIRCLE_SAMPLES);
        self.apply_claim(player, disk).claimed_area
    }

    pub fn clear_owner(&mut self, player: CompetitorId) -> f32 {
        let old = self.area(player);
        if old > 0.0 {
            self.territories[player.index()] = MultiPolygon::empty();
            self.rebuild_index();
            self.revision = self.revision.wrapping_add(1);
        }
        old
    }

    pub fn calculate_capture(
        &self,
        player: CompetitorId,
        trail: &ActiveTrail,
        trail_width: f32,
    ) -> VectorCaptureResult {
        let trail_points = trail_points(trail);
        let corridor = stroke_polyline(&trail_points, trail_width);
        if corridor.is_empty() {
            return VectorCaptureResult::default();
        }

        let mut claim = corridor.clone();
        let mut used_loop_fill = false;
        let territory = &self.territories[player.index()];
        if let Some(loop_shape) = self.loop_candidate(territory, &trail_points) {
            claim = loop_shape.union(&corridor);
            used_loop_fill = true;
        }
        claim = claim.intersection(&self.arena);
        let before = self.area(player);
        let after = territory.union(&claim).area();
        let claimed_area = (after - before).max(0.0);
        let stolen_by_owner = self
            .territories
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != player.index())
            .map(|(index, other)| {
                let stolen = other.intersection(&claim).area();
                (CompetitorId(index as u8), stolen)
            })
            .filter(|(_, area)| *area > 1e-5)
            .collect();

        VectorCaptureResult {
            claim,
            claimed_area,
            stolen_by_owner,
            used_loop_fill,
        }
    }

    pub fn apply_claim(
        &mut self,
        player: CompetitorId,
        claim: MultiPolygon,
    ) -> VectorCaptureResult {
        let claim = claim.intersection(&self.arena);
        let before = self.area(player);
        let stolen_by_owner: Vec<_> = self
            .territories
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != player.index())
            .map(|(index, other)| (CompetitorId(index as u8), other.intersection(&claim).area()))
            .filter(|(_, area)| *area > 1e-5)
            .collect();
        for (index, territory) in self.territories.iter_mut().enumerate() {
            if index != player.index() {
                *territory = territory.difference(&claim);
            }
        }
        self.territories[player.index()] = self.territories[player.index()].union(&claim);
        self.rebuild_index();
        let claimed_area = (self.area(player) - before).max(0.0);
        if claimed_area > 1e-5 || !stolen_by_owner.is_empty() {
            self.revision = self.revision.wrapping_add(1);
        }
        VectorCaptureResult {
            claim,
            claimed_area,
            stolen_by_owner,
            used_loop_fill: false,
        }
    }

    /// Applies all captures from the same simulation instant in stable player
    /// order. Each later claim sees the already committed earlier claim, so
    /// contested slivers cannot be double-owned.
    pub fn apply_equal_time_captures(
        &mut self,
        captures: &mut [(CompetitorId, VectorCaptureResult)],
    ) {
        captures.sort_by_key(|(player, _)| *player);
        let mut committed = MultiPolygon::empty();
        for (player, result) in captures {
            let claim = result.claim.difference(&committed);
            result.claim = claim.clone();
            let applied = self.apply_claim(*player, claim);
            result.claimed_area = applied.claimed_area;
            result.stolen_by_owner = applied.stolen_by_owner;
            committed = committed.union(&result.claim);
        }
    }

    /// Rebuilds the low-resolution sample cache after a geometry commit. This
    /// is presentation/broadphase data; no gameplay query depends on it.
    pub fn rebuild_sample_cache(&self, board: &mut BoardGrid) {
        let mut owner_counts = [0_u32; MAX_COMPETITORS];
        let previous = board.owner.clone();
        // Keep the derived invalidation queue bounded even in headless/server
        // worlds that do not have the render sync system consuming it.
        board.ownership_changes.clear();
        board.owner.fill(OwnerId::UNCLAIMED);
        for index in 0..board.len() {
            if !board.field_mask[index] {
                continue;
            }
            let owner = self.owner_at(board.cell_center(board.cell(index)));
            board.owner[index] = owner;
            if let Some(player) = owner.competitor() {
                owner_counts[player.index()] += 1;
            }
        }
        board.owner_counts = owner_counts;
        board.owned_cells = std::array::from_fn(|_| Vec::new());
        board.owner_cell_slots.fill(u32::MAX);
        for (index, owner) in board.owner.iter().copied().enumerate() {
            if let Some(player) = owner.competitor() {
                let slot = board.owned_cells[player.index()].len() as u32;
                board.owned_cells[player.index()].push(index);
                board.owner_cell_slots[index] = slot;
            }
            if previous.get(index).copied() != Some(owner) {
                board.ownership_changes.push(crate::board::OwnershipChange {
                    index,
                    old: previous.get(index).copied().unwrap_or(OwnerId::UNCLAIMED),
                    new: owner,
                });
            }
        }
        board.ownership_revision = board.ownership_revision.wrapping_add(1);
        board.dirty_chunks.fill(true);
    }

    fn loop_candidate(&self, territory: &MultiPolygon, trail: &[Vec2]) -> Option<MultiPolygon> {
        let start = *trail.first()?;
        let end = *trail.last()?;
        let mut best: Option<MultiPolygon> = None;
        for polygon in &territory.polygons {
            let Some(start_anchor) = nearest_contour_anchor(&polygon.outer, start) else {
                continue;
            };
            let Some(end_anchor) = nearest_contour_anchor(&polygon.outer, end) else {
                continue;
            };
            let Some(first) = closed_loop(trail, &polygon.outer, start_anchor, end_anchor, false)
            else {
                continue;
            };
            let Some(second) = closed_loop(trail, &polygon.outer, start_anchor, end_anchor, true)
            else {
                continue;
            };
            for candidate in [first, second] {
                let candidate = MultiPolygon::from_contour(candidate);
                // The lobe is the region on the outside of the current turf;
                // clipping it to the player's territory would erase the very
                // area the capture is supposed to add.
                let clipped = candidate.intersection(&self.arena);
                if clipped.is_empty() {
                    continue;
                }
                if best
                    .as_ref()
                    .is_none_or(|current| clipped.area() < current.area())
                {
                    best = Some(clipped);
                }
            }
        }
        best
    }

    fn rebuild_index(&mut self) {
        self.index = TerritorySpatialIndex::build(&self.arena, &self.territories);
    }
}

const INDEX_SIDE: usize = 64;

#[derive(Clone, Debug, Default, PartialEq)]
struct TerritorySpatialIndex {
    origin: Vec2,
    cell_size: Vec2,
    cells: Vec<Vec<u8>>,
}

impl TerritorySpatialIndex {
    fn build(arena: &MultiPolygon, territories: &[MultiPolygon; MAX_COMPETITORS]) -> Self {
        let Some((min, max)) = arena.bounds() else {
            return Self::default();
        };
        let origin = min.world();
        let extent = (max.world() - origin).max(Vec2::splat(1.0));
        let cell_size = extent / INDEX_SIDE as f32;
        let mut index = Self {
            origin,
            cell_size,
            cells: vec![Vec::new(); INDEX_SIDE * INDEX_SIDE],
        };
        for (owner, territory) in territories.iter().enumerate() {
            let Some((min, max)) = territory.bounds() else {
                continue;
            };
            let (a, b) = index.bounds(min.world(), max.world());
            for y in a.1..=b.1 {
                for x in a.0..=b.0 {
                    let slot = y * INDEX_SIDE + x;
                    if !index.cells[slot].contains(&(owner as u8)) {
                        index.cells[slot].push(owner as u8);
                    }
                }
            }
        }
        index
    }

    fn bounds(&self, min: Vec2, max: Vec2) -> ((usize, usize), (usize, usize)) {
        let to_cell = |point: Vec2| {
            let relative = (point - self.origin) / self.cell_size;
            (
                relative.x.floor().clamp(0.0, (INDEX_SIDE - 1) as f32) as usize,
                relative.y.floor().clamp(0.0, (INDEX_SIDE - 1) as f32) as usize,
            )
        };
        (to_cell(min), to_cell(max))
    }

    fn candidates(&self, point: Vec2) -> &[u8] {
        if self.cells.is_empty()
            || point.x < self.origin.x
            || point.y < self.origin.y
            || point.x > self.origin.x + self.cell_size.x * INDEX_SIDE as f32
            || point.y > self.origin.y + self.cell_size.y * INDEX_SIDE as f32
        {
            return &[];
        }
        let relative = (point - self.origin) / self.cell_size;
        let x = relative.x.floor().clamp(0.0, (INDEX_SIDE - 1) as f32) as usize;
        let y = relative.y.floor().clamp(0.0, (INDEX_SIDE - 1) as f32) as usize;
        &self.cells[y * INDEX_SIDE + x]
    }
}

fn circle(center: Vec2, radius: f32, samples: usize) -> MultiPolygon {
    let points = (0..samples)
        .map(|index| {
            let angle = std::f32::consts::TAU * index as f32 / samples as f32;
            Point::from_world(center + Vec2::from_angle(angle) * radius)
        })
        .collect();
    MultiPolygon::from_contour(points)
}

fn trail_points(trail: &ActiveTrail) -> Vec<Vec2> {
    let mut points = trail.points.clone();
    if points
        .last()
        .is_none_or(|point| point.distance_squared(trail.head) > 1e-8)
    {
        points.push(trail.head);
    }
    points
}

fn stroke_polyline(points: &[Vec2], width: f32) -> MultiPolygon {
    let radius = width.max(0.01) * 0.5;
    let mut contours = Vec::with_capacity(points.len().saturating_mul(3));
    for segment in points.windows(2) {
        let a = segment[0];
        let b = segment[1];
        let direction = (b - a).normalize_or_zero();
        let normal = Vec2::new(-direction.y, direction.x) * radius;
        contours.push(
            [a - normal, b - normal, b + normal, a + normal]
                .into_iter()
                .map(Point::from_world)
                .collect(),
        );
        contours.push(circle_contour(a, radius, STROKE_SAMPLES));
        contours.push(circle_contour(b, radius, STROKE_SAMPLES));
    }
    if points.len() == 1 {
        contours.push(circle_contour(points[0], radius, STROKE_SAMPLES));
    }
    MultiPolygon::from_union_contours(contours)
}

fn circle_contour(center: Vec2, radius: f32, samples: usize) -> Vec<Point> {
    (0..samples)
        .map(|index| {
            let angle = std::f32::consts::TAU * index as f32 / samples as f32;
            Point::from_world(center + Vec2::from_angle(angle) * radius)
        })
        .collect()
}

#[derive(Clone, Copy)]
struct ContourAnchor {
    edge: usize,
    position: f32,
    point: Vec2,
}

fn nearest_contour_anchor(contour: &[Point], point: Vec2) -> Option<ContourAnchor> {
    let mut best = None;
    for index in 0..contour.len() {
        let a = contour[index].world();
        let b = contour[(index + 1) % contour.len()].world();
        let direction = b - a;
        let t = if direction.length_squared() > 1e-8 {
            ((point - a).dot(direction) / direction.length_squared()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let candidate = a + direction * t;
        let distance = candidate.distance_squared(point);
        if best.is_none_or(|(_, current)| distance < current) {
            best = Some((
                ContourAnchor {
                    edge: index,
                    position: t,
                    point: candidate,
                },
                distance,
            ));
        }
    }
    best.map(|(anchor, _)| anchor)
}

fn closed_loop(
    trail: &[Vec2],
    contour: &[Point],
    start: ContourAnchor,
    end: ContourAnchor,
    forward: bool,
) -> Option<Vec<Point>> {
    if trail.len() < 2 || contour.len() < 3 {
        return None;
    }
    let mut points: Vec<Point> = trail.iter().copied().map(Point::from_world).collect();
    if let Some(first) = points.first_mut() {
        *first = Point::from_world(start.point);
    }
    if let Some(last) = points.last_mut() {
        *last = Point::from_world(end.point);
    }

    if forward {
        if end.edge != start.edge || end.position > start.position {
            let mut index = (end.edge + 1) % contour.len();
            loop {
                points.push(contour[index]);
                if index == start.edge {
                    break;
                }
                index = (index + 1) % contour.len();
                if points.len() > trail.len() + contour.len() {
                    return None;
                }
            }
        }
    } else if end.edge != start.edge || end.position < start.position {
        let mut index = end.edge;
        let last = (start.edge + 1) % contour.len();
        loop {
            points.push(contour[index]);
            if index == last {
                break;
            }
            index = (index + contour.len() - 1) % contour.len();
            if points.len() > trail.len() + contour.len() {
                return None;
            }
        }
    }
    Some(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arena() -> MultiPolygon {
        MultiPolygon::from_outer(&[
            Vec2::new(-20.0, -20.0),
            Vec2::new(20.0, -20.0),
            Vec2::new(20.0, 20.0),
            Vec2::new(-20.0, 20.0),
        ])
    }

    #[test]
    fn vector_map_claims_and_steals_without_cells() {
        let mut map = TerritoryMap::new(arena());
        let a = CompetitorId(0);
        let b = CompetitorId(1);
        map.claim_disk(Vec2::new(-3.0, 0.0), 3.0, a);
        map.claim_disk(Vec2::new(3.0, 0.0), 3.0, b);
        let before = map.area(b);
        let claim = MultiPolygon::from_outer(&[
            Vec2::new(0.0, -2.0),
            Vec2::new(5.0, -2.0),
            Vec2::new(5.0, 2.0),
            Vec2::new(0.0, 2.0),
        ]);
        let result = map.apply_claim(a, claim);
        assert!(result.claimed_area > 0.0);
        assert!(map.area(b) < before);
        assert_eq!(map.owner_at(Vec2::new(4.0, 0.0)), a.owner());
    }

    #[test]
    fn claims_are_always_clipped_to_the_arena() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        let oversized = MultiPolygon::from_outer(&[
            Vec2::new(-30.0, -30.0),
            Vec2::new(30.0, -30.0),
            Vec2::new(30.0, 30.0),
            Vec2::new(-30.0, 30.0),
        ]);

        map.apply_claim(player, oversized);

        assert_eq!(map.area(player), map.arena_area);
        assert!(!map.owns(Vec2::new(25.0, 0.0), player));
    }

    #[test]
    fn victory_threshold_uses_fixed_point_area_at_the_boundary() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        map.territories[player.index()] = MultiPolygon::from_outer(&[
            Vec2::new(-20.0, -20.0),
            Vec2::new(17.99, -20.0),
            Vec2::new(17.99, 20.0),
            Vec2::new(-20.0, 20.0),
        ]);
        assert!(!map.reaches_victory_threshold(player, 95));

        map.territories[player.index()] = MultiPolygon::from_outer(&[
            Vec2::new(-20.0, -20.0),
            Vec2::new(18.0, -20.0),
            Vec2::new(18.0, 20.0),
            Vec2::new(-20.0, 20.0),
        ]);
        assert!(map.reaches_victory_threshold(player, 95));
    }

    #[test]
    fn capture_can_reach_victory_without_full_conquest() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        let opponent = CompetitorId(1);
        map.apply_claim(
            opponent,
            MultiPolygon::from_outer(&[
                Vec2::new(18.0, -20.0),
                Vec2::new(20.0, -20.0),
                Vec2::new(20.0, 20.0),
                Vec2::new(18.0, 20.0),
            ]),
        );
        map.apply_claim(
            player,
            MultiPolygon::from_outer(&[
                Vec2::new(-20.0, -20.0),
                Vec2::new(18.0, -20.0),
                Vec2::new(18.0, 20.0),
                Vec2::new(-20.0, 20.0),
            ]),
        );

        assert!(map.reaches_victory_threshold(player, 95));
        assert!(map.area(opponent) > 0.0);
    }

    #[test]
    fn areas_are_stable_under_repeated_normalization() {
        let mut map = TerritoryMap::new(arena());
        map.claim_disk(Vec2::ZERO, 4.0, CompetitorId(0));
        let first = map.area(CompetitorId(0));
        for _ in 0..3 {
            map.territories[0] = map.territories[0].normalize();
        }
        assert!((map.area(CompetitorId(0)) - first).abs() < 0.001);
    }

    #[test]
    fn returning_to_vector_boundary_fills_a_lobe_without_sampling_cells() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        map.territories[player.index()] = MultiPolygon::from_outer(&[
            Vec2::new(-8.0, -8.0),
            Vec2::new(8.0, -8.0),
            Vec2::new(8.0, 8.0),
            Vec2::new(-8.0, 8.0),
        ]);
        map.rebuild_index();
        let start_cell = crate::board::Cell::new(0, 0);
        let mut trail = ActiveTrail::new(player, start_cell, Vec2::new(-8.0, -4.0), Vec2::Y);
        trail.append_exact(Vec2::new(-4.0, -12.0));
        trail.append_exact(Vec2::new(4.0, -12.0));
        trail.append_exact(Vec2::new(8.0, -4.0));
        let result = map.calculate_capture(player, &trail, 0.6);
        assert!(result.used_loop_fill);
        assert!(result.claimed_area > 20.0);
        assert!(result.claim.contains_world(Vec2::new(0.0, -10.0)));
    }

    #[test]
    fn same_edge_closure_uses_the_short_boundary_segment() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        map.territories[player.index()] = MultiPolygon::from_outer(&[
            Vec2::new(-8.0, -8.0),
            Vec2::new(8.0, -8.0),
            Vec2::new(8.0, 8.0),
            Vec2::new(-8.0, 8.0),
        ]);
        map.rebuild_index();
        let mut trail = ActiveTrail::new(
            player,
            crate::board::Cell::new(0, 0),
            Vec2::new(-4.0, -8.0),
            -Vec2::Y,
        );
        trail.append_exact(Vec2::new(0.0, -12.0));
        trail.append_exact(Vec2::new(4.0, -8.0));

        let result = map.calculate_capture(player, &trail, 0.6);

        assert!(result.used_loop_fill);
        assert!(result.claim.contains_world(Vec2::new(0.0, -10.0)));
        assert!(!result.claim.contains_world(Vec2::new(0.0, 0.0)));
    }
}
