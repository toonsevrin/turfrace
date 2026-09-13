//! Authoritative vector territory state.
//!
//! The sample grid is intentionally absent from this module.  It is only
//! rebuilt after a committed geometry change for broadphase/render helpers;
//! containment, capture, scoring and elimination all use this map directly.

use bevy::prelude::*;

use crate::{
    board::{BoardGrid, Cell},
    geometry::{MultiPolygon, Point},
    ids::{CompetitorId, MAX_COMPETITORS, OwnerId},
    trail::ActiveTrail,
};

mod arena_boundary;
mod containment_index;
mod frontier_index;
mod spatial_index;

pub use arena_boundary::ArenaBoundary;
use arena_boundary::ArenaDistanceCache;
use frontier_index::FrontierIndex;
use spatial_index::TerritorySpatialIndex;

const CIRCLE_SAMPLES: usize = 32;
const STROKE_SAMPLES: usize = 12;
const MAX_OWNERSHIP_CHANGES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OwnerFrontier {
    pub position: Vec2,
    pub outward: Vec2,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorCaptureResult {
    pub claim: MultiPolygon,
    pub claimed_area: f32,
    pub stolen_by_owner: Vec<(CompetitorId, f32)>,
    pub used_loop_fill: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct TerritorySamples {
    width: u32,
    height: u32,
    cell_size: f32,
    origin: Vec2,
    field_mask: Vec<bool>,
    owners: Vec<OwnerId>,
    owner_counts: [u32; MAX_COMPETITORS],
    owned_cells: [Vec<usize>; MAX_COMPETITORS],
    owner_cell_slots: Vec<u32>,
    frontier_bits: Vec<u16>,
    frontier_index: FrontierIndex,
    revision: u64,
    changes: Vec<OwnershipChange>,
    changes_overflowed: bool,
}

impl Default for TerritorySamples {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            cell_size: 0.5,
            origin: Vec2::ZERO,
            field_mask: Vec::new(),
            owners: Vec::new(),
            owner_counts: [0; MAX_COMPETITORS],
            owned_cells: std::array::from_fn(|_| Vec::new()),
            owner_cell_slots: Vec::new(),
            frontier_bits: Vec::new(),
            frontier_index: FrontierIndex::default(),
            revision: 0,
            changes: Vec::new(),
            changes_overflowed: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnershipChange {
    pub index: usize,
    pub old: OwnerId,
    pub new: OwnerId,
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct TerritoryMap {
    arena: MultiPolygon,
    arena_distance_cache: ArenaDistanceCache,
    territories: [MultiPolygon; MAX_COMPETITORS],
    arena_area: f32,
    revision: u64,
    index: TerritorySpatialIndex,
    samples: TerritorySamples,
}

impl Default for TerritoryMap {
    fn default() -> Self {
        Self::new(MultiPolygon::empty())
    }
}

impl TerritorySamples {
    fn from_board(board: &BoardGrid) -> Self {
        let len = board.len();
        let mut samples = Self {
            width: board.width,
            height: board.height,
            cell_size: board.cell_size,
            origin: board.world_origin,
            field_mask: board.field_mask.clone(),
            owners: vec![OwnerId::UNCLAIMED; len],
            owner_counts: [0; MAX_COMPETITORS],
            owned_cells: std::array::from_fn(|_| Vec::new()),
            owner_cell_slots: vec![u32::MAX; len],
            frontier_bits: vec![0; len],
            frontier_index: FrontierIndex::default(),
            revision: 1,
            changes: Vec::with_capacity(8),
            changes_overflowed: false,
        };
        samples.rebuild_frontiers();
        samples
    }

    fn is_empty(&self) -> bool {
        self.owners.is_empty()
    }

    fn index(&self, cell: Cell) -> Option<usize> {
        (cell.x >= 0 && cell.y >= 0 && cell.x < self.width as i32 && cell.y < self.height as i32)
            .then(|| cell.y as usize * self.width as usize + cell.x as usize)
    }

    fn cell(&self, index: usize) -> Cell {
        Cell::new(
            index as i32 % self.width as i32,
            index as i32 / self.width as i32,
        )
    }

    fn cell_center(&self, cell: Cell) -> Vec2 {
        self.origin + Vec2::new(cell.x as f32 + 0.5, cell.y as f32 + 0.5) * self.cell_size
    }

    fn clamped_cell_bounds(&self, min: Vec2, max: Vec2) -> Option<(Cell, Cell)> {
        if self.is_empty() {
            return None;
        }
        let min_relative = (min - self.origin) / self.cell_size;
        let max_relative = (max - self.origin) / self.cell_size;
        Some((
            Cell::new(
                (min_relative.x.floor() as i32).clamp(0, self.width as i32 - 1),
                (min_relative.y.floor() as i32).clamp(0, self.height as i32 - 1),
            ),
            Cell::new(
                (max_relative.x.floor() as i32).clamp(0, self.width as i32 - 1),
                (max_relative.y.floor() as i32).clamp(0, self.height as i32 - 1),
            ),
        ))
    }

    fn owner_at(&self, cell: Cell) -> OwnerId {
        self.index(cell)
            .filter(|&index| self.field_mask[index])
            .map_or(OwnerId::UNCLAIMED, |index| self.owners[index])
    }

    fn nearest_owner_frontier(
        &self,
        position: Vec2,
        player: CompetitorId,
        maximum_distance: f32,
    ) -> Option<Vec2> {
        let (minimum, maximum) = self.clamped_cell_bounds(
            position - Vec2::splat(maximum_distance),
            position + Vec2::splat(maximum_distance),
        )?;
        let bit = 1u16 << player.index();
        let max_distance_squared = maximum_distance * maximum_distance;
        let mut best = None;
        for &index in self.frontier_index.owner_indices(player.index()) {
            let cell = self.cell(index);
            if cell.x < minimum.x || cell.x > maximum.x || cell.y < minimum.y || cell.y > maximum.y
            {
                continue;
            }
            let center = self.cell_center(cell);
            if self.frontier_bits[index] & bit != 0
                && center.distance_squared(position) <= max_distance_squared
                && best.is_none_or(|(_, distance)| center.distance_squared(position) < distance)
            {
                best = Some((center, center.distance_squared(position)));
            }
        }
        best.map(|(center, _)| center)
    }

    fn collect_owner_frontiers(
        &self,
        position: Vec2,
        player: CompetitorId,
        maximum_distance: f32,
        output: &mut Vec<OwnerFrontier>,
    ) {
        output.clear();
        let Some((minimum, maximum)) = self.clamped_cell_bounds(
            position - Vec2::splat(maximum_distance),
            position + Vec2::splat(maximum_distance),
        ) else {
            return;
        };
        let bit = 1u16 << player.index();
        let maximum_distance_squared = maximum_distance * maximum_distance;
        for &index in self.frontier_index.owner_indices(player.index()) {
            let cell = self.cell(index);
            if cell.x < minimum.x || cell.x > maximum.x || cell.y < minimum.y || cell.y > maximum.y
            {
                continue;
            }
            let x = cell.x;
            let y = cell.y;
            if self.frontier_bits[index] & bit == 0 {
                continue;
            }
            let center = self.cell_center(cell);
            if center.distance_squared(position) > maximum_distance_squared {
                continue;
            }
            let mut outward = Vec2::ZERO;
            for (neighbor, direction) in [
                (Cell::new(x - 1, y), -Vec2::X),
                (Cell::new(x + 1, y), Vec2::X),
                (Cell::new(x, y - 1), -Vec2::Y),
                (Cell::new(x, y + 1), Vec2::Y),
            ] {
                if self.owner_at(neighbor) != player.owner() {
                    outward += direction;
                }
            }
            output.push(OwnerFrontier {
                position: center,
                outward: outward.normalize_or((center - position).normalize_or(Vec2::Y)),
            });
        }
    }

    /// Updates ownership bookkeeping; callers refresh frontiers after the
    /// complete batch, so adjacent mutations do not repeatedly rebuild halos.
    fn set_owner(&mut self, index: usize, owner: OwnerId) -> bool {
        if !self.field_mask[index] || self.owners[index] == owner {
            return false;
        }
        let old = self.owners[index];
        if let Some(previous) = old.competitor() {
            self.owner_counts[previous.index()] -= 1;
            self.remove_owned_cell(previous, index);
        }
        self.owners[index] = owner;
        if let Some(player) = owner.competitor() {
            self.owner_counts[player.index()] += 1;
            self.owner_cell_slots[index] = self.owned_cells[player.index()].len() as u32;
            self.owned_cells[player.index()].push(index);
        } else {
            self.owner_cell_slots[index] = u32::MAX;
        }
        // Presentation may be absent (for example in a headless world), so
        // this queue is a bounded hint rather than authoritative state. The
        // sample revision and owners remain authoritative when entries drop.
        if !self.changes_overflowed {
            if self.changes.len() < MAX_OWNERSHIP_CHANGES {
                self.changes.push(OwnershipChange {
                    index,
                    old,
                    new: owner,
                });
            } else {
                // A partial queue cannot be applied safely: the renderer
                // falls back to a bounded diff of the authoritative samples.
                self.changes.clear();
                self.changes_overflowed = true;
            }
        }
        true
    }

    fn remove_owned_cell(&mut self, owner: CompetitorId, index: usize) {
        let slot = self.owner_cell_slots[index];
        let cells = &mut self.owned_cells[owner.index()];
        if slot == u32::MAX || slot as usize >= cells.len() || cells[slot as usize] != index {
            return;
        }
        let last = cells.pop().unwrap();
        if last != index {
            cells[slot as usize] = last;
            self.owner_cell_slots[last] = slot;
        }
        self.owner_cell_slots[index] = u32::MAX;
    }

    fn refresh_frontiers_around(&mut self, changed: &[usize]) {
        let mut dirty = Vec::with_capacity(changed.len().saturating_mul(5));
        for &index in changed {
            let cell = self.cell(index);
            for candidate in [
                cell,
                Cell::new(cell.x - 1, cell.y),
                Cell::new(cell.x + 1, cell.y),
                Cell::new(cell.x, cell.y - 1),
                Cell::new(cell.x, cell.y + 1),
            ] {
                if let Some(index) = self.index(candidate) {
                    dirty.push(index);
                }
            }
        }
        dirty.sort_unstable();
        dirty.dedup();
        for index in dirty {
            self.refresh_frontier_index(index);
        }
    }

    fn refresh_frontier_index(&mut self, index: usize) {
        let old_bits = self.frontier_bits[index];
        let new_bits = if let Some(owner) = self.owners[index].competitor() {
            let cell = self.cell(index);
            let frontier = [
                Cell::new(cell.x - 1, cell.y),
                Cell::new(cell.x + 1, cell.y),
                Cell::new(cell.x, cell.y - 1),
                Cell::new(cell.x, cell.y + 1),
            ]
            .into_iter()
            .any(|neighbor| self.owner_at(neighbor) != owner.owner());
            if frontier { 1u16 << owner.index() } else { 0 }
        } else {
            0
        };
        self.frontier_bits[index] = new_bits;
        self.frontier_index.update(index, old_bits, new_bits);
    }

    fn rebuild_frontiers(&mut self) {
        self.frontier_bits.fill(0);
        self.frontier_index.clear();
        for index in 0..self.owners.len() {
            self.refresh_frontier_index(index);
        }
    }
}

impl TerritoryMap {
    pub fn new(arena: MultiPolygon) -> Self {
        let arena_area = arena.area();
        let arena_distance_cache = ArenaDistanceCache::new(&arena);
        Self {
            arena,
            arena_distance_cache,
            territories: std::array::from_fn(|_| MultiPolygon::empty()),
            arena_area,
            revision: 1,
            index: TerritorySpatialIndex::default(),
            samples: TerritorySamples::default(),
        }
    }

    pub fn from_board(board: &BoardGrid) -> Self {
        let mut map = Self::new(MultiPolygon::from_outer(&board.contour.points));
        map.samples = TerritorySamples::from_board(board);
        map
    }

    pub fn arena(&self) -> &MultiPolygon {
        &self.arena
    }

    pub fn territory(&self, player: CompetitorId) -> &MultiPolygon {
        &self.territories[player.index()]
    }

    pub fn territories(&self) -> &[MultiPolygon; MAX_COMPETITORS] {
        &self.territories
    }

    pub fn arena_area(&self) -> f32 {
        self.arena_area
    }

    /// Returns the exact fixed-point footprint used by a seed claim. Keeping
    /// this constructor next to `seed_owner` prevents respawn validation from
    /// drifting from the geometry that is eventually committed.
    pub fn seed_footprint(center: Vec2, radius: f32) -> MultiPolygon {
        circle(center, radius, CIRCLE_SAMPLES)
    }

    /// Tests a complete seed site against authoritative vector geometry.
    ///
    /// This deliberately does not consult the sample cache: even a very thin
    /// positive overlap with turf rejects the site, as does any portion of the
    /// disk outside the arena.
    pub fn is_neutral_seed_site(&self, center: Vec2, radius: f32) -> bool {
        if !center.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return false;
        }
        let disk = Self::seed_footprint(center, radius);
        if disk.is_empty() || !disk.difference(&self.arena).is_empty() {
            return false;
        }
        self.territories
            .iter()
            .all(|territory| disk.intersection(territory).is_empty())
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn sample_owners(&self) -> &[OwnerId] {
        &self.samples.owners
    }

    pub fn sample_owner_counts(&self) -> &[u32; MAX_COMPETITORS] {
        &self.samples.owner_counts
    }

    pub fn sample_ownership_revision(&self) -> u64 {
        self.samples.revision
    }

    pub fn sample_dimensions(&self) -> (u32, u32, f32, Vec2) {
        (
            self.samples.width,
            self.samples.height,
            self.samples.cell_size,
            self.samples.origin,
        )
    }

    pub fn sample_cell_size(&self) -> f32 {
        self.samples.cell_size
    }

    pub fn sample_owner_at(&self, cell: Cell) -> OwnerId {
        self.samples.owner_at(cell)
    }

    pub fn sample_owner_at_world(&self, point: Vec2) -> OwnerId {
        let relative = (point - self.samples.origin) / self.samples.cell_size;
        self.samples.owner_at(Cell::new(
            relative.x.floor() as i32,
            relative.y.floor() as i32,
        ))
    }

    pub fn nearest_owner_frontier(
        &self,
        position: Vec2,
        player: CompetitorId,
        maximum_distance: f32,
    ) -> Option<Vec2> {
        self.samples
            .nearest_owner_frontier(position, player, maximum_distance)
    }

    pub fn collect_owner_frontiers(
        &self,
        position: Vec2,
        player: CompetitorId,
        maximum_distance: f32,
        output: &mut Vec<OwnerFrontier>,
    ) {
        self.samples
            .collect_owner_frontiers(position, player, maximum_distance, output);
    }

    /// Drains the compact sample invalidation queue for presentation. Simulation
    /// never needs to consume this queue, and no mutable sample storage escapes.
    pub fn drain_ownership_changes(&mut self, output: &mut Vec<OwnershipChange>) {
        output.clear();
        if self.samples.changes_overflowed {
            self.samples.changes_overflowed = false;
            return;
        }
        output.append(&mut self.samples.changes);
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
        let mask = self.index.candidate_mask(point).unwrap_or(u16::MAX);
        let point = Point::from_world(point);
        for index in 0..MAX_COMPETITORS {
            if mask & (1 << index) != 0 && self.territory_contains(index, point) {
                return CompetitorId(index as u8).owner();
            }
        }
        OwnerId::UNCLAIMED
    }

    pub fn owns(&self, point: Vec2, player: CompetitorId) -> bool {
        let mask = self.index.candidate_mask(point).unwrap_or(u16::MAX);
        mask & (1 << player.index()) != 0
            && self.territory_contains(player.index(), Point::from_world(point))
    }

    fn territory_contains(&self, index: usize, point: Point) -> bool {
        self.index
            .contains(index, point)
            .unwrap_or_else(|| self.territories[index].contains(point))
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
        ArenaBoundary::new(&self.arena, &self.arena_distance_cache)
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
        let disk = Self::seed_footprint(center, radius);
        self.apply_claim(player, disk)
    }

    pub fn clear_owner(&mut self, player: CompetitorId) -> f32 {
        let old = self.area(player);
        if old > 0.0 {
            // Only samples previously assigned to this owner can change.
            // Re-query them rather than assuming UNCLAIMED: an exact shared
            // boundary may now belong to a surviving owner.
            let mut affected = self.samples.owned_cells[player.index()].clone();
            affected.sort_unstable();
            self.territories[player.index()] = MultiPolygon::empty();
            self.rebuild_index();
            self.refresh_sample_indices(affected.into_iter());
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
        if claim.is_empty() {
            return VectorCaptureResult::default();
        }
        let before = self.area(player);
        let claim_bounds = claim.bounds();
        let overlaps: [bool; MAX_COMPETITORS] = std::array::from_fn(|index| {
            bounds_overlap(self.territories[index].bounds(), claim_bounds)
        });
        let before_areas: [i64; MAX_COMPETITORS] =
            std::array::from_fn(|index| self.territories[index].area_scaled());
        let stolen_by_owner: Vec<_> = self
            .territories
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != player.index())
            .filter(|(index, _)| overlaps[*index])
            .map(|(index, other)| (CompetitorId(index as u8), other.intersection(&claim).area()))
            .filter(|(_, area)| *area > 1e-5)
            .collect();
        for (index, territory) in self.territories.iter_mut().enumerate() {
            if index != player.index() && overlaps[index] {
                *territory = territory.difference(&claim);
            }
        }
        self.territories[player.index()] = self.territories[player.index()].union(&claim);
        self.rebuild_index();
        let claimed_area = (self.area(player) - before).max(0.0);
        let geometry_changed = self
            .territories
            .iter()
            .enumerate()
            .any(|(index, territory)| territory.area_scaled() != before_areas[index]);
        if geometry_changed {
            self.refresh_sample_cache(&claim);
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

    /// Refreshes only samples covered by the changed geometry. Exact vector
    /// geometry remains authoritative; the cache is for rendering and NPC
    /// broadphase queries. Claims therefore never trigger a full-arena scan.
    fn refresh_sample_cache(&mut self, changed: &MultiPolygon) {
        let Some((min, max)) = changed.bounds() else {
            return;
        };
        let Some((min, max)) = self.samples.clamped_cell_bounds(min.world(), max.world()) else {
            return;
        };
        let width = self.samples.width as usize;
        self.refresh_sample_indices(
            (min.y..=max.y)
                .flat_map(move |y| (min.x..=max.x).map(move |x| y as usize * width + x as usize)),
        );
    }

    fn refresh_sample_indices(&mut self, indices: impl Iterator<Item = usize>) {
        let mut changed = Vec::new();
        for index in indices {
            let owner = self.owner_at(self.samples.cell_center(self.samples.cell(index)));
            if self.samples.set_owner(index, owner) {
                changed.push(index);
            }
        }
        if !changed.is_empty() {
            self.samples.refresh_frontiers_around(&changed);
            self.samples.revision = self.samples.revision.wrapping_add(1);
        }
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

fn bounds_overlap(left: Option<(Point, Point)>, right: Option<(Point, Point)>) -> bool {
    match (left, right) {
        (Some((a, b)), Some((c, d))) => a.x <= d.x && c.x <= b.x && a.y <= d.y && c.y <= b.y,
        _ => false,
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
    fn bridge_capture_is_vector_corridor_only() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        map.seed_owner(Vec2::new(-6.0, 0.0), 3.0, player);
        // Build an intentionally disconnected fixture to exercise the
        // corridor-only geometry fallback.
        map.territories[player.index()] =
            map.territories[player.index()].union(&MultiPolygon::from_outer(&[
                Vec2::new(3.0, -3.0),
                Vec2::new(9.0, -3.0),
                Vec2::new(9.0, 3.0),
                Vec2::new(3.0, 3.0),
            ]));
        let mut trail = ActiveTrail::new(
            player,
            crate::board::Cell::new(0, 0),
            Vec2::new(-3.0, 0.0),
            Vec2::X,
        );
        trail.append_exact(Vec2::new(3.0, 0.0));
        let result = map.calculate_capture(player, &trail, 0.6);
        assert!(!result.used_loop_fill);
        assert!(result.claimed_area > 0.0);
        assert!(result.claim.contains_world(Vec2::ZERO));
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
        let board = BoardGrid::generate(31, 2, &config);
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
        let before_area = map.area(opponent);
        let claim = rectangle(Vec2::new(14.0, -1.0), Vec2::new(16.0, 1.0));
        map.apply_claim(player, claim);

        assert!(map.area(opponent) > 0.0);
        assert!(map.area(opponent) < before_area);
        let counts = map.sample_owner_counts();
        assert_eq!(
            counts[player.index()],
            map.sample_owners()
                .iter()
                .filter(|owner| **owner == player.owner())
                .count() as u32
        );
        assert_eq!(
            counts[opponent.index()],
            map.sample_owners()
                .iter()
                .filter(|owner| **owner == opponent.owner())
                .count() as u32
        );
        assert!(map.sample_ownership_revision() > 1);
    }

    #[test]
    fn atomic_claim_updates_samples_frontiers_and_change_revision() {
        let config = crate::config::GameConfig::default();
        let board = BoardGrid::generate(19, 2, &config);
        let mut map = TerritoryMap::from_board(&board);
        let player = CompetitorId(0);
        map.seed_owner(Vec2::ZERO, 3.0, player);
        let mut changes = Vec::new();
        map.drain_ownership_changes(&mut changes);
        let before = map.sample_ownership_revision();

        map.apply_claim(
            player,
            rectangle(Vec2::new(-5.0, -1.0), Vec2::new(5.0, 1.0)),
        );

        assert!(map.sample_ownership_revision() > before);
        map.drain_ownership_changes(&mut changes);
        assert!(!changes.is_empty());
        assert!(
            map.nearest_owner_frontier(Vec2::ZERO, player, 8.0)
                .is_some()
        );
    }

    #[test]
    fn sparse_frontier_queries_match_row_major_brute_force_after_mutations() {
        let config = crate::config::GameConfig::default();
        let board = BoardGrid::generate(41, 2, &config);
        let mut map = TerritoryMap::from_board(&board);
        let players = [CompetitorId(0), CompetitorId(1), CompetitorId(2)];
        let positions = [
            Vec2::ZERO,
            Vec2::new(-6.0, 0.0),
            Vec2::new(6.0, 0.0),
            Vec2::new(0.0, 4.25),
            Vec2::new(-40.0, -40.0),
        ];
        let radii = [0.0, 0.25, 1.0, 3.0, 8.0, 100.0];

        let assert_equivalent = |map: &TerritoryMap| {
            for owner in players {
                let expected_indices: Vec<_> = (0..map.samples.owners.len())
                    .filter(|&index| {
                        map.samples.frontier_bits[index] & (1u16 << owner.index()) != 0
                    })
                    .collect();
                assert_eq!(
                    map.samples
                        .frontier_index
                        .owner_indices(owner.index())
                        .copied()
                        .collect::<Vec<_>>(),
                    expected_indices
                );
                for position in positions {
                    for maximum_distance in radii {
                        assert_eq!(
                            map.nearest_owner_frontier(position, owner, maximum_distance),
                            brute_nearest_owner_frontier(
                                &map.samples,
                                position,
                                owner,
                                maximum_distance,
                            )
                        );
                        let mut actual = Vec::new();
                        let mut expected = Vec::new();
                        map.collect_owner_frontiers(position, owner, maximum_distance, &mut actual);
                        brute_collect_owner_frontiers(
                            &map.samples,
                            position,
                            owner,
                            maximum_distance,
                            &mut expected,
                        );
                        assert_eq!(actual, expected);
                    }
                }
            }
        };

        assert_equivalent(&map);
        map.seed_owner(Vec2::new(-6.0, 0.0), 3.0, players[0]);
        assert_equivalent(&map);
        map.seed_owner(Vec2::new(6.0, 0.0), 3.0, players[1]);
        assert_equivalent(&map);
        map.apply_claim(
            players[0],
            rectangle(Vec2::new(-10.0, -1.0), Vec2::new(10.0, 1.0)),
        );
        assert_equivalent(&map);
        map.clear_owner(players[1]);
        assert_equivalent(&map);
        map.apply_claim(
            players[2],
            rectangle(Vec2::new(2.0, 2.0), Vec2::new(10.0, 8.0)),
        );
        assert_equivalent(&map);
    }

    fn brute_nearest_owner_frontier(
        samples: &TerritorySamples,
        position: Vec2,
        player: CompetitorId,
        maximum_distance: f32,
    ) -> Option<Vec2> {
        let (minimum, maximum) = samples.clamped_cell_bounds(
            position - Vec2::splat(maximum_distance),
            position + Vec2::splat(maximum_distance),
        )?;
        let bit = 1u16 << player.index();
        let maximum_distance_squared = maximum_distance * maximum_distance;
        let mut best = None;
        for y in minimum.y..=maximum.y {
            for x in minimum.x..=maximum.x {
                let cell = Cell::new(x, y);
                let index = samples.index(cell).expect("clamped frontier cell");
                let center = samples.cell_center(cell);
                if samples.frontier_bits[index] & bit != 0
                    && center.distance_squared(position) <= maximum_distance_squared
                    && best.is_none_or(|(_, distance)| center.distance_squared(position) < distance)
                {
                    best = Some((center, center.distance_squared(position)));
                }
            }
        }
        best.map(|(center, _)| center)
    }

    fn brute_collect_owner_frontiers(
        samples: &TerritorySamples,
        position: Vec2,
        player: CompetitorId,
        maximum_distance: f32,
        output: &mut Vec<OwnerFrontier>,
    ) {
        output.clear();
        let Some((minimum, maximum)) = samples.clamped_cell_bounds(
            position - Vec2::splat(maximum_distance),
            position + Vec2::splat(maximum_distance),
        ) else {
            return;
        };
        let bit = 1u16 << player.index();
        let maximum_distance_squared = maximum_distance * maximum_distance;
        for y in minimum.y..=maximum.y {
            for x in minimum.x..=maximum.x {
                let cell = Cell::new(x, y);
                let index = samples.index(cell).expect("clamped frontier cell");
                if samples.frontier_bits[index] & bit == 0 {
                    continue;
                }
                let center = samples.cell_center(cell);
                if center.distance_squared(position) > maximum_distance_squared {
                    continue;
                }
                let mut outward = Vec2::ZERO;
                for (neighbor, direction) in [
                    (Cell::new(x - 1, y), -Vec2::X),
                    (Cell::new(x + 1, y), Vec2::X),
                    (Cell::new(x, y - 1), -Vec2::Y),
                    (Cell::new(x, y + 1), Vec2::Y),
                ] {
                    if samples.owner_at(neighbor) != player.owner() {
                        outward += direction;
                    }
                }
                output.push(OwnerFrontier {
                    position: center,
                    outward: outward.normalize_or((center - position).normalize_or(Vec2::Y)),
                });
            }
        }
    }

    #[test]
    fn claim_bounds_rejection_matches_unconditional_booleans() {
        let config = crate::config::GameConfig::default();
        let board = BoardGrid::generate(23, 3, &config);
        let mut map = TerritoryMap::from_board(&board);
        map.seed_owner(Vec2::new(-10.0, 0.0), 3.0, CompetitorId(0));
        map.seed_owner(Vec2::new(10.0, 0.0), 3.0, CompetitorId(1));
        for center in [Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(10.0, 0.0)] {
            let claim = circle(center, 3.0, CIRCLE_SAMPLES).intersection(map.arena());
            let expected: [MultiPolygon; MAX_COMPETITORS] = std::array::from_fn(|index| {
                if index == 2 {
                    map.territories[index].union(&claim)
                } else {
                    map.territories[index].difference(&claim)
                }
            });
            map.apply_claim(CompetitorId(2), claim);
            assert_eq!(map.territories, expected);
        }
    }

    #[test]
    fn clearing_owner_reassigns_shared_boundary_samples() {
        let config = crate::config::GameConfig::default();
        let board = BoardGrid::generate(23, 2, &config);
        let mut map = TerritoryMap::from_board(&board);
        let cell = board.world_to_cell(Vec2::ZERO).unwrap();
        let index = map.samples.index(cell).unwrap();
        let center = map.samples.cell_center(cell);
        for (owner, min_x, max_x) in [(0, center.x - 3.0, center.x), (1, center.x, center.x + 3.0)]
        {
            map.apply_claim(
                CompetitorId(owner),
                MultiPolygon::from_outer(&[
                    Vec2::new(min_x, center.y - 3.0),
                    Vec2::new(max_x, center.y - 3.0),
                    Vec2::new(max_x, center.y + 3.0),
                    Vec2::new(min_x, center.y + 3.0),
                ]),
            );
        }
        assert!(map.owns(center, CompetitorId(0)) && map.owns(center, CompetitorId(1)));
        assert_eq!(map.samples.owners[index], CompetitorId(0).owner());
        map.clear_owner(CompetitorId(0));
        assert_eq!(map.samples.owners[index], CompetitorId(1).owner());
    }

    #[test]
    fn sparse_owner_clear_matches_full_bounds_refresh() {
        let config = crate::config::GameConfig::default();
        let board = BoardGrid::generate(23, 3, &config);
        let mut map = TerritoryMap::from_board(&board);
        map.seed_owner(Vec2::ZERO, 8.0, CompetitorId(0));
        map.seed_owner(Vec2::new(6.0, 0.0), 5.0, CompetitorId(1));
        map.seed_owner(Vec2::new(-6.0, 0.0), 5.0, CompetitorId(2));
        for owner in [CompetitorId(0), CompetitorId(1), CompetitorId(2)] {
            let mut reference = map.clone();
            let changed = reference.territories[owner.index()].clone();
            reference.territories[owner.index()] = MultiPolygon::empty();
            reference.rebuild_index();
            reference.refresh_sample_cache(&changed);
            reference.revision = reference.revision.wrapping_add(1);
            map.clear_owner(owner);
            assert_eq!(map, reference);
        }
    }

    #[test]
    fn headless_change_queue_is_bounded_and_reports_overflow_for_diffing() {
        let config = crate::config::GameConfig::default();
        let board = BoardGrid::generate(23, 2, &config);
        let mut map = TerritoryMap::from_board(&board);
        let mut changed = 0;
        for index in 0..map.samples.owners.len() {
            if map.samples.field_mask[index]
                && map.samples.set_owner(index, CompetitorId(0).owner())
            {
                changed += 1;
                if changed > MAX_OWNERSHIP_CHANGES {
                    break;
                }
            }
        }
        assert_eq!(changed, MAX_OWNERSHIP_CHANGES + 1);
        let mut changes = Vec::new();
        map.drain_ownership_changes(&mut changes);
        assert!(changes.is_empty());
    }

    #[test]
    fn atomic_noop_claim_does_not_advance_revisions_or_queue_changes() {
        let config = crate::config::GameConfig::default();
        let board = BoardGrid::generate(17, 2, &config);
        let mut map = TerritoryMap::from_board(&board);
        let player = CompetitorId(0);
        map.seed_owner(Vec2::ZERO, 3.0, player);
        let map_revision = map.revision();
        let sample_revision = map.sample_ownership_revision();
        let mut changes = Vec::new();
        map.drain_ownership_changes(&mut changes);

        map.apply_claim(
            player,
            rectangle(Vec2::new(-1.0, -1.0), Vec2::new(1.0, 1.0)),
        );

        assert_eq!(map.revision(), map_revision);
        assert_eq!(map.sample_ownership_revision(), sample_revision);
        map.drain_ownership_changes(&mut changes);
        assert!(changes.is_empty());
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
    fn neutral_seed_validation_rejects_thin_exact_slivers_and_boundary_escape() {
        let mut map = TerritoryMap::new(arena());
        let sliver_owner = CompetitorId(0);
        map.apply_claim(
            sliver_owner,
            rectangle(Vec2::new(2.9, -0.01), Vec2::new(3.2, 0.01)),
        );
        assert!(!map.is_neutral_seed_site(Vec2::ZERO, 3.0));
        assert!(!map.is_neutral_seed_site(Vec2::new(18.0, 0.0), 3.0));
        assert!(map.is_neutral_seed_site(Vec2::new(-10.0, 0.0), 3.0));
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
