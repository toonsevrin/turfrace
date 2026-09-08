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

mod arena_boundary;
mod spatial_index;

pub use arena_boundary::ArenaBoundary;
use spatial_index::TerritorySpatialIndex;

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
        if let Some(mask) = self.index.candidate_mask(point) {
            for index in 0..MAX_COMPETITORS {
                if mask & (1 << index) != 0 && self.territories[index].contains_world(point) {
                    return CompetitorId(index as u8).owner();
                }
            }
            return OwnerId::UNCLAIMED;
        }
        for (index, territory) in self.territories.iter().enumerate() {
            if territory.contains_world(point) {
                return CompetitorId(index as u8).owner();
            }
        }
        OwnerId::UNCLAIMED
    }

    pub fn owns(&self, point: Vec2, player: CompetitorId) -> bool {
        self.index.candidate_mask(point).map_or_else(
            || self.territories[player.index()].contains_world(point),
            |mask| {
                mask & (1 << player.index()) != 0
                    && self.territories[player.index()].contains_world(point)
            },
        )
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

    pub fn arena_boundary(&self) -> ArenaBoundary<'_> {
        ArenaBoundary::new(&self.arena)
    }

    pub fn arena_signed_distance(&self, point: Vec2) -> f32 {
        self.arena_boundary().signed_distance(point)
    }

    pub fn arena_inward_normal(&self, point: Vec2) -> Vec2 {
        self.arena_boundary().inward_normal(point)
    }

    /// Establishes a fresh spawn seed for an owner with no existing territory.
    ///
    /// Seeding is a lifecycle transition, not a general-purpose claim mode:
    /// callers must clear the owner first.
    pub fn seed_owner(
        &mut self,
        center: Vec2,
        radius: f32,
        player: CompetitorId,
    ) -> VectorCaptureResult {
        assert!(
            self.territories[player.index()].is_empty(),
            "cannot seed an owner that still has territory"
        );
        let disk = circle(center, radius, CIRCLE_SAMPLES);
        self.apply_claim(player, disk)
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
        let mut result = self.prepare_capture(player, trail, trail_width);
        let territory = &self.territories[player.index()];
        let before = territory.area();
        result.claimed_area = (territory.union(&result.claim).area() - before).max(0.0);
        result.stolen_by_owner = self
            .territories
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != player.index())
            .map(|(index, other)| {
                let stolen = other.intersection(&result.claim).area();
                (CompetitorId(index as u8), stolen)
            })
            .filter(|(_, area)| *area > 1e-5)
            .collect();
        result
    }

    /// Builds capture geometry without measuring speculative ownership.
    ///
    /// The simulation commits every pending claim immediately afterward, at
    /// which point [`Self::apply_claim`] computes authoritative area and theft
    /// metrics. Keeping that hot path geometry-only avoids repeating one union
    /// and up to seven opponent intersections at every trail closure.
    pub(crate) fn prepare_capture(
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

        VectorCaptureResult {
            claim,
            claimed_area: 0.0,
            stolen_by_owner: Vec::new(),
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
        let capture_count = captures.len();
        for (index, (player, result)) in captures.iter_mut().enumerate() {
            // The overwhelmingly common one-player closure needs no
            // arbitration geometry. In a simultaneous group, the first claim
            // likewise has nothing to subtract, and the final committed union
            // would never be observed.
            if !committed.is_empty() {
                result.claim = result.claim.difference(&committed);
            }
            let applied = self.apply_claim(*player, result.claim.clone());
            result.claimed_area = applied.claimed_area;
            result.stolen_by_owner = applied.stolen_by_owner;
            if index + 1 < capture_count {
                if committed.is_empty() {
                    committed.clone_from(&result.claim);
                } else {
                    committed = committed.union(&result.claim);
                }
            }
        }
    }

    /// Refreshes only cells covered by geometry that can have changed.
    ///
    /// Exact vector geometry remains authoritative. This cache exists for
    /// rendering and broadphase queries, so a local capture must not trigger a
    /// full-arena polygon containment pass.
    pub fn refresh_sample_cache(&self, board: &mut BoardGrid, changed: &MultiPolygon) {
        let Some((min, max)) = changed.bounds() else {
            return;
        };
        self.refresh_sample_cache_bounds(board, min.world(), max.world());
    }

    fn refresh_sample_cache_bounds(&self, board: &mut BoardGrid, min: Vec2, max: Vec2) {
        let Some((min, max)) = board.clamped_cell_bounds(min, max) else {
            return;
        };
        for y in min.y..=max.y {
            for x in min.x..=max.x {
                let cell = crate::board::Cell::new(x, y);
                let Some(index) = board.index(cell) else {
                    continue;
                };
                if board.field_mask[index] {
                    board.set_owner_index(index, self.owner_at(board.cell_center(cell)));
                }
            }
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
        board.rebuild_owner_frontiers();
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
            // Both ends must belong to this island. Snapping a trail to an
            // unrelated nearest contour can manufacture a remote capture (or
            // select a smaller, wrong lobe when an owner has several islands).
            // Allow only fixed-point/boundary-crossing roundoff outside it.
            let touches = |point: Vec2, anchor: ContourAnchor| {
                polygon.contains_world(point) || point.distance_squared(anchor.point) <= 0.0001
            };
            if !touches(start, start_anchor) || !touches(end, end_anchor) {
                continue;
            }
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
        self.index.rebuild(&self.arena, &self.territories);
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
    let mut contours = Vec::with_capacity(points.len().saturating_mul(2).saturating_sub(1));
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
    }
    // One disk per unique sample joins adjacent quads and rounds both ends.
    // Adding a disk for both endpoints of every segment duplicated every
    // interior contour and made long-trail overlay work almost twice as large.
    for &point in points {
        contours.push(circle_contour(point, radius, STROKE_SAMPLES));
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

    #[test]
    fn claim_only_removes_the_geometry_it_covers_and_preserves_islands() {
        let mut map = TerritoryMap::new(arena());
        let victim = CompetitorId(0);
        let attacker = CompetitorId(1);
        let left = rectangle(Vec2::new(-8.0, -3.0), Vec2::new(-2.0, 3.0));
        let bridge = rectangle(Vec2::new(-2.0, -0.6), Vec2::new(2.0, 0.6));
        let right = rectangle(Vec2::new(2.0, -3.0), Vec2::new(8.0, 3.0));
        map.territories[victim.index()] = left.union(&bridge).union(&right);
        map.rebuild_index();
        let before_right_area = map.area(victim);

        let result = map.apply_claim(
            attacker,
            rectangle(Vec2::new(-0.4, -2.0), Vec2::new(0.4, 2.0)),
        );

        assert!(
            result
                .stolen_by_owner
                .iter()
                .any(|(owner, area)| { *owner == victim && *area > 0.0 })
        );
        assert!(map.owns(Vec2::new(-5.0, 0.0), victim));
        assert!(map.owns(Vec2::new(5.0, 0.0), victim));
        assert_eq!(map.territories[victim.index()].polygons.len(), 2);
        assert!(map.area(victim) > before_right_area - 3.0);
    }

    #[test]
    fn stealing_the_anchor_area_does_not_erase_the_remainder() {
        let mut map = TerritoryMap::new(arena());
        let victim = CompetitorId(0);
        let attacker = CompetitorId(1);
        map.territories[victim.index()] = rectangle(Vec2::new(-8.0, -2.0), Vec2::new(8.0, 2.0));
        map.rebuild_index();

        map.apply_claim(
            attacker,
            rectangle(Vec2::new(-6.0, -3.0), Vec2::new(-4.0, 3.0)),
        );

        assert!(!map.owns(Vec2::new(-5.0, 0.0), victim));
        assert!(map.owns(Vec2::new(5.0, 0.0), victim));
        assert!(!map.territories[victim.index()].is_empty());
    }

    #[test]
    fn clear_owner_removes_all_islands_for_respawn_lifecycle() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        map.territories[player.index()] = rectangle(Vec2::new(-8.0, -2.0), Vec2::new(8.0, 2.0));
        assert!(map.clear_owner(player) > 0.0);
        assert!(map.territories[player.index()].is_empty());
    }

    #[test]
    fn disconnected_island_can_close_a_trail() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        let main = rectangle(Vec2::new(-8.0, -8.0), Vec2::new(8.0, 8.0));
        let island = rectangle(Vec2::new(12.0, -3.0), Vec2::new(16.0, 3.0));
        map.territories[player.index()] = main.union(&island);
        map.rebuild_index();

        let mut trail = ActiveTrail::new(
            player,
            crate::board::Cell::new(0, 0),
            Vec2::new(12.0, -3.0),
            -Vec2::Y,
        );
        trail.append_exact(Vec2::new(12.0, -10.0));
        trail.append_exact(Vec2::new(16.0, -10.0));
        trail.append_exact(Vec2::new(16.0, -3.0));

        let result = map.calculate_capture(player, &trail, 0.6);

        assert!(result.used_loop_fill);
        assert!(result.claim.contains_world(Vec2::new(14.0, -7.0)));
    }

    #[test]
    fn joining_separate_islands_claims_only_the_corridor() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        map.apply_claim(
            player,
            rectangle(Vec2::new(-8.0, -2.0), Vec2::new(-4.0, 2.0)),
        );
        map.apply_claim(player, rectangle(Vec2::new(4.0, -2.0), Vec2::new(8.0, 2.0)));
        let mut trail = ActiveTrail::new(
            player,
            crate::board::Cell::new(0, 0),
            Vec2::new(-4.0, 0.0),
            Vec2::X,
        );
        trail.append_exact(Vec2::new(0.0, -8.0));
        trail.append_exact(Vec2::new(4.0, 0.0));
        let result = map.calculate_capture(player, &trail, 0.6);
        assert!(!result.used_loop_fill);
        assert!(result.claim.contains_world(Vec2::new(0.0, -8.0)));
        assert!(!result.claim.contains_world(Vec2::new(0.0, -3.0)));
    }

    #[test]
    fn sample_cache_and_records_follow_geometry_after_island_capture() {
        let config = crate::config::GameConfig::default();
        let mut board = BoardGrid::generate(31, 2, &config);
        let mut map = TerritoryMap::from_board(&board);
        let player = CompetitorId(0);
        let opponent = CompetitorId(1);
        map.apply_claim(
            player,
            rectangle(Vec2::new(-10.0, -4.0), Vec2::new(10.0, 4.0)),
        );
        map.apply_claim(
            opponent,
            rectangle(Vec2::new(12.0, -4.0), Vec2::new(18.0, 4.0)),
        );
        map.rebuild_sample_cache(&mut board);

        let before_area = map.area(opponent);
        let claim = rectangle(Vec2::new(14.0, -1.0), Vec2::new(16.0, 1.0));
        let result = map.apply_claim(player, claim);
        map.refresh_sample_cache(&mut board, &result.claim);

        assert!(map.area(opponent) > 0.0);
        assert!(map.area(opponent) < before_area);
        assert_eq!(
            board.owner_counts[player.index()],
            board.owned_cells[player.index()].len() as u32
        );
        assert_eq!(
            board.owner_counts[opponent.index()],
            board.owned_cells[opponent.index()].len() as u32
        );
        assert!(board.verify_counts());
    }

    #[test]
    #[should_panic(expected = "cannot seed an owner that still has territory")]
    fn seeding_requires_the_previous_lifecycle_to_be_cleared() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        map.seed_owner(Vec2::new(-6.0, 0.0), 2.0, player);

        map.seed_owner(Vec2::new(6.0, 0.0), 2.0, player);
    }

    #[test]
    fn spatial_index_uses_compact_candidate_masks() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        map.seed_owner(Vec2::new(-6.0, 0.0), 2.0, player);
        let allocation = map.index.cells.as_ptr();

        map.apply_claim(
            player,
            rectangle(Vec2::new(-6.0, -1.0), Vec2::new(6.0, 1.0)),
        );
        assert_eq!(map.index.cells.len(), spatial_index::INDEX_CELL_COUNT);
        assert_eq!(map.index.cells.as_ptr(), allocation);
        assert_eq!(
            map.index.candidate_mask(Vec2::new(-6.0, 0.0)),
            Some(1 << player.index())
        );
        assert_eq!(map.index.candidate_mask(Vec2::new(15.0, 15.0)), Some(0));
    }

    fn rectangle(min: Vec2, max: Vec2) -> MultiPolygon {
        MultiPolygon::from_outer(&[min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)])
    }

    fn arena() -> MultiPolygon {
        MultiPolygon::from_outer(&[
            Vec2::new(-20.0, -20.0),
            Vec2::new(20.0, -20.0),
            Vec2::new(20.0, 20.0),
            Vec2::new(-20.0, 20.0),
        ])
    }

    #[test]
    fn arena_projection_satisfies_margin_and_rejects_invalid_input() {
        let map = TerritoryMap::new(arena());
        let projected = map
            .arena_boundary()
            .project_inside(Vec2::new(21.0, 19.8), 0.5)
            .unwrap();

        assert!(map.arena_signed_distance(projected) >= 0.5);
        let margin_point = map
            .arena_boundary()
            .project_to_margin(Vec2::new(18.0, 19.0), 0.5)
            .unwrap();
        assert!((map.arena_signed_distance(margin_point) - 0.5).abs() <= 0.01);
        assert_eq!(
            map.arena_boundary().project_inside(Vec2::ZERO, 0.5),
            Some(Vec2::ZERO)
        );
        assert_eq!(map.arena_boundary().project_inside(Vec2::NAN, 0.5), None);
        assert_eq!(map.arena_boundary().project_inside(Vec2::ZERO, -0.5), None);
    }

    #[test]
    fn arena_margin_exit_time_finds_the_last_valid_segment_point() {
        let map = TerritoryMap::new(arena());
        let from = Vec2::ZERO;
        let to = Vec2::new(25.0, 0.0);
        let margin = 0.5;
        let time = map
            .arena_boundary()
            .margin_exit_time(from, to, margin)
            .unwrap();
        let crossing = from.lerp(to, time);

        assert!((map.arena_signed_distance(crossing) - margin).abs() <= 0.01);
        assert_eq!(
            map.arena_boundary().margin_exit_time(to, from, margin),
            None
        );
    }

    #[test]
    fn vector_map_claims_and_steals_without_cells() {
        let mut map = TerritoryMap::new(arena());
        let a = CompetitorId(0);
        let b = CompetitorId(1);
        map.seed_owner(Vec2::new(-3.0, 0.0), 3.0, a);
        map.seed_owner(Vec2::new(3.0, 0.0), 3.0, b);
        let before = map.area(b);
        let claim = MultiPolygon::from_outer(&[
            Vec2::new(-1.0, -2.0),
            Vec2::new(5.0, -2.0),
            Vec2::new(5.0, 2.0),
            Vec2::new(-1.0, 2.0),
        ]);
        let result = map.apply_claim(a, claim);
        assert!(result.claimed_area > 0.0);
        assert!(map.area(b) < before);
        assert_eq!(map.owner_at(Vec2::new(4.0, 0.0)), a.owner());
    }

    #[test]
    fn single_equal_time_capture_keeps_geometry_and_commits_metrics() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        let claim = rectangle(Vec2::new(-3.0, -2.0), Vec2::new(3.0, 2.0));
        let mut captures = [(player, VectorCaptureResult { claim, ..default() })];

        map.apply_equal_time_captures(&mut captures);

        assert!(captures[0].1.claim.contains_world(Vec2::ZERO));
        assert!(captures[0].1.claimed_area > 0.0);
        assert!(map.owns(Vec2::ZERO, player));
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
        map.seed_owner(Vec2::ZERO, 4.0, CompetitorId(0));
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
    fn prepared_capture_matches_fully_calculated_capture_after_commit() {
        let mut calculated_map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        let victim = CompetitorId(1);
        calculated_map.seed_owner(Vec2::new(-6.0, 0.0), 4.0, player);
        calculated_map.seed_owner(Vec2::new(0.0, 0.0), 3.0, victim);
        let mut prepared_map = calculated_map.clone();
        let mut trail = ActiveTrail::new(
            player,
            crate::board::Cell::new(0, 0),
            Vec2::new(-4.0, -2.0),
            Vec2::X,
        );
        trail.append_exact(Vec2::new(2.0, -2.0));

        let calculated = calculated_map.calculate_capture(player, &trail, 0.6);
        let prepared = prepared_map.prepare_capture(player, &trail, 0.6);
        assert_eq!(prepared.claimed_area, 0.0);
        assert!(prepared.stolen_by_owner.is_empty());

        let calculated_applied = calculated_map.apply_claim(player, calculated.claim);
        let prepared_applied = prepared_map.apply_claim(player, prepared.claim);
        assert!((calculated_applied.claimed_area - prepared_applied.claimed_area).abs() < 0.001);
        assert_eq!(
            calculated_applied.stolen_by_owner.len(),
            prepared_applied.stolen_by_owner.len()
        );
        assert!((calculated_map.area(player) - prepared_map.area(player)).abs() < 0.001);
        assert!((calculated_map.area(victim) - prepared_map.area(victim)).abs() < 0.001);
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
