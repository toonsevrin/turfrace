use std::collections::VecDeque;

use bevy::prelude::*;

use crate::{
    config::GameConfig,
    ids::{CompetitorId, MAX_COMPETITORS},
};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Cell {
    pub x: i32,
    pub y: i32,
}

impl Cell {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FieldContour {
    pub points: Vec<Vec2>,
    pub radial_samples: Vec<f32>,
}

/// A conservative spatial reference to one authoritative trail segment.
///
/// References are kept in the board cells touched by a segment. They let
/// collision detection reject distant trail history before entering the
/// continuous narrow phase. A segment may occur in more than one cell, so
/// callers must deduplicate references before testing them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrailSegmentRef {
    pub owner: CompetitorId,
    pub segment: usize,
}

#[derive(Resource, Clone, Debug)]
pub struct BoardGrid {
    pub width: u32,
    pub height: u32,
    pub cell_size: f32,
    pub world_origin: Vec2,
    pub field_mask: Vec<bool>,
    pub active_trail_bits: Vec<u16>,
    pub trail_segment_buckets: Vec<Vec<TrailSegmentRef>>,
    pub signed_distance: Vec<f32>,
    pub spawn_candidates: Vec<usize>,
    pub contour: FieldContour,
    pub playable_cells: u32,
    /// Stable generation identity for contour/field-mask presentation caches.
    pub generation_revision: u64,
}

impl Default for BoardGrid {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            cell_size: 0.5,
            world_origin: Vec2::ZERO,
            field_mask: Vec::new(),
            active_trail_bits: Vec::new(),
            trail_segment_buckets: Vec::new(),
            signed_distance: Vec::new(),
            spawn_candidates: Vec::new(),
            contour: FieldContour::default(),
            playable_cells: 0,
            generation_revision: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DeterministicRng(u64);

impl DeterministicRng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    pub fn unit_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u32 << 24) as f32
    }
    pub fn range_f32(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.unit_f32()
    }
    pub fn index(&mut self, len: usize) -> usize {
        (self.next_u64() % len as u64) as usize
    }
}

impl BoardGrid {
    pub fn generate(seed: u64, competitors: usize, config: &GameConfig) -> Self {
        let count = competitors.clamp(2, MAX_COMPETITORS);
        let target_area = 4000.0 + 700.0 * count as f32;
        let equivalent_radius = (target_area / std::f32::consts::PI).sqrt();
        let mut rng = DeterministicRng::new(seed);
        let p = rng.range_f32(2.2, 3.6);
        let aspect = rng.range_f32(0.88, 1.12);
        let a = equivalent_radius * aspect.sqrt();
        let b = equivalent_radius / aspect.sqrt();
        let harmonics: Vec<(usize, f32, f32)> = (2..=6)
            .map(|k| {
                (
                    k,
                    rng.range_f32(-0.018, 0.018),
                    rng.range_f32(0.0, std::f32::consts::TAU),
                )
            })
            .collect();
        let mut radii = Vec::with_capacity(128);
        for sample in 0..128 {
            let theta = std::f32::consts::TAU * sample as f32 / 128.0;
            let denom = (theta.cos().abs() / a).powf(p) + (theta.sin().abs() / b).powf(p);
            let base = 1.0 / denom.powf(1.0 / p);
            let deformation = (1.0
                + harmonics
                    .iter()
                    .map(|(k, amp, phase)| amp * (*k as f32 * theta + phase).cos())
                    .sum::<f32>())
            .clamp(0.88, 1.12);
            radii.push(base * deformation);
        }
        let mut points = radial_points(&radii);
        let area = polygon_area(&points).abs();
        let scale = (target_area / area).sqrt();
        for radius in &mut radii {
            *radius *= scale;
        }
        for point in &mut points {
            *point *= scale;
        }
        let max = radii.iter().copied().fold(0.0_f32, f32::max) + config.cell_size * 2.0;
        let extent = (max / config.cell_size).ceil() as i32;
        let width = (extent * 2 + 1) as u32;
        let height = width;
        let origin = Vec2::splat(-(extent as f32) * config.cell_size);
        let len = (width * height) as usize;
        let contour = FieldContour {
            points,
            radial_samples: radii,
        };
        let mut field_mask = vec![false; len];
        let mut signed_distance = vec![0.0; len];
        let mut playable_cells = 0;
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                let idx = (y as u32 * width + x as u32) as usize;
                let position = origin
                    + Vec2::new(
                        (x as f32 + 0.5) * config.cell_size,
                        (y as f32 + 0.5) * config.cell_size,
                    );
                let (inside, edge_distance) = radial_polygon_sample(position, &contour.points);
                field_mask[idx] = inside;
                if inside {
                    playable_cells += 1;
                }
                signed_distance[idx] = if inside {
                    edge_distance
                } else {
                    -edge_distance
                };
            }
        }
        let spawn_candidates = (0..len)
            .filter(|&index| {
                field_mask[index] && signed_distance[index] >= config.spawn_boundary_clearance
            })
            .collect();
        Self {
            width,
            height,
            cell_size: config.cell_size,
            world_origin: origin,
            field_mask,
            active_trail_bits: vec![0; len],
            trail_segment_buckets: vec![Vec::new(); len],
            signed_distance,
            spawn_candidates,
            contour,
            playable_cells,
            generation_revision: seed ^ (count as u64).rotate_left(32),
        }
    }

    pub fn len(&self) -> usize {
        self.field_mask.len()
    }
    pub fn is_empty(&self) -> bool {
        self.field_mask.is_empty()
    }
    pub fn index(&self, cell: Cell) -> Option<usize> {
        (cell.x >= 0 && cell.y >= 0 && cell.x < self.width as i32 && cell.y < self.height as i32)
            .then(|| cell.y as usize * self.width as usize + cell.x as usize)
    }
    pub fn cell(&self, index: usize) -> Cell {
        Cell::new(
            index as i32 % self.width as i32,
            index as i32 / self.width as i32,
        )
    }
    pub fn world_to_cell(&self, position: Vec2) -> Option<Cell> {
        let relative = (position - self.world_origin) / self.cell_size;
        let cell = Cell::new(relative.x.floor() as i32, relative.y.floor() as i32);
        self.index(cell).map(|_| cell)
    }

    /// Returns the board-clamped cell rectangle covering a world-space AABB.
    /// Unlike `world_to_cell`, this remains useful when the AABB crosses the
    /// edge of the board.
    pub fn clamped_cell_bounds(&self, min: Vec2, max: Vec2) -> Option<(Cell, Cell)> {
        if self.is_empty() {
            return None;
        }
        let min_relative = (min - self.world_origin) / self.cell_size;
        let max_relative = (max - self.world_origin) / self.cell_size;
        let last_x = self.width as i32 - 1;
        let last_y = self.height as i32 - 1;
        Some((
            Cell::new(
                (min_relative.x.floor() as i32).clamp(0, last_x),
                (min_relative.y.floor() as i32).clamp(0, last_y),
            ),
            Cell::new(
                (max_relative.x.floor() as i32).clamp(0, last_x),
                (max_relative.y.floor() as i32).clamp(0, last_y),
            ),
        ))
    }
    pub fn cell_center(&self, cell: Cell) -> Vec2 {
        self.world_origin + Vec2::new(cell.x as f32 + 0.5, cell.y as f32 + 0.5) * self.cell_size
    }
    pub fn is_playable(&self, cell: Cell) -> bool {
        self.index(cell).is_some_and(|i| self.field_mask[i])
    }
    pub fn signed_distance_at(&self, position: Vec2) -> f32 {
        self.world_to_cell(position)
            .and_then(|c| self.index(c))
            .map_or(-f32::INFINITY, |i| self.signed_distance[i])
    }
    pub fn nearest_interior(&self, position: Vec2, margin: f32) -> Vec2 {
        if self.signed_distance_at(position) >= margin {
            return position;
        }
        self.nearest_cell_center(position, |index| {
            self.field_mask[index] && self.signed_distance[index] >= margin
        })
        .unwrap_or(Vec2::ZERO)
    }
    fn nearest_cell_center(
        &self,
        position: Vec2,
        predicate: impl Fn(usize) -> bool,
    ) -> Option<Vec2> {
        if self.is_empty() {
            return None;
        }
        let relative = (position - self.world_origin) / self.cell_size;
        let start = Cell::new(
            (relative.x.floor() as i32).clamp(0, self.width as i32 - 1),
            (relative.y.floor() as i32).clamp(0, self.height as i32 - 1),
        );
        let maximum_radius = self.width.max(self.height) as i32;
        let mut best: Option<(Vec2, f32)> = None;
        for radius in 0..=maximum_radius {
            let mut consider = |cell: Cell| {
                let Some(index) = self.index(cell) else {
                    return;
                };
                if !predicate(index) {
                    return;
                }
                let center = self.cell_center(cell);
                let distance = center.distance_squared(position);
                if best.is_none_or(|(_, current)| distance < current) {
                    best = Some((center, distance));
                }
            };
            for x in start.x - radius..=start.x + radius {
                consider(Cell::new(x, start.y - radius));
                if radius > 0 {
                    consider(Cell::new(x, start.y + radius));
                }
            }
            for y in start.y - radius + 1..start.y + radius {
                consider(Cell::new(start.x - radius, y));
                if radius > 0 {
                    consider(Cell::new(start.x + radius, y));
                }
            }
            if let Some((center, distance)) = best {
                let next_ring_lower_bound = radius as f32 * self.cell_size;
                if next_ring_lower_bound * next_ring_lower_bound >= distance {
                    return Some(center);
                }
            }
        }
        best.map(|(center, _)| center)
    }
    pub fn inward_normal(&self, position: Vec2) -> Vec2 {
        let e = self.cell_size;
        let dx = self.signed_distance_at(position + Vec2::X * e)
            - self.signed_distance_at(position - Vec2::X * e);
        let dy = self.signed_distance_at(position + Vec2::Y * e)
            - self.signed_distance_at(position - Vec2::Y * e);
        Vec2::new(dx, dy)
            .try_normalize()
            .unwrap_or_else(|| -position.try_normalize().unwrap_or(Vec2::Y))
    }
    pub fn collect_nearby_trail_segments(
        &self,
        position: Vec2,
        radius: f32,
        output: &mut Vec<TrailSegmentRef>,
    ) {
        output.clear();
        self.visit_nearby_trail_segments(position, radius, |reference| output.push(reference));
        output.sort_unstable_by_key(|reference| (reference.owner.0, reference.segment));
        output.dedup();
    }

    /// Visit raw bucket references in canonical cell order, allowing bounded
    /// selectors to filter before allocating. References may repeat across cells.
    pub(crate) fn visit_nearby_trail_segments(
        &self,
        position: Vec2,
        radius: f32,
        mut visit: impl FnMut(TrailSegmentRef),
    ) {
        let Some((minimum, maximum)) = self.clamped_cell_bounds(
            position - Vec2::splat(radius),
            position + Vec2::splat(radius),
        ) else {
            return;
        };
        for y in minimum.y..=maximum.y {
            for x in minimum.x..=maximum.x {
                let Some(index) = self.index(Cell::new(x, y)) else {
                    continue;
                };
                for &reference in &self.trail_segment_buckets[index] {
                    visit(reference);
                }
            }
        }
    }
    pub fn connected_playable(&self) -> bool {
        let Some(start) = self.field_mask.iter().position(|inside| *inside) else {
            return false;
        };
        let mut seen = vec![false; self.len()];
        let mut queue = VecDeque::from([start]);
        seen[start] = true;
        let mut total = 0;
        while let Some(i) = queue.pop_front() {
            total += 1;
            let c = self.cell(i);
            for n in [
                Cell::new(c.x - 1, c.y),
                Cell::new(c.x + 1, c.y),
                Cell::new(c.x, c.y - 1),
                Cell::new(c.x, c.y + 1),
            ] {
                if let Some(j) = self.index(n)
                    && self.field_mask[j]
                    && !seen[j]
                {
                    seen[j] = true;
                    queue.push_back(j);
                }
            }
        }
        total == self.playable_cells
    }
}

/// Samples a star-shaped radial polygon without scanning every edge.
///
/// Generated fields have evenly spaced polar vertices. A ray from the origin
/// therefore intersects the edge for its angular sector, and the nearest
/// boundary for points close enough to affect gameplay lies in that sector or
/// one of its immediate neighbours. Deep interior/exterior points only need a
/// conservative distance because callers compare it with small clearances.
fn radial_polygon_sample(position: Vec2, points: &[Vec2]) -> (bool, f32) {
    debug_assert!(points.len() >= 3);
    if position.length_squared() <= f32::EPSILON {
        return (
            true,
            points
                .iter()
                .map(|point| point.length())
                .fold(f32::INFINITY, f32::min),
        );
    }

    let count = points.len();
    let angle = position
        .y
        .atan2(position.x)
        .rem_euclid(std::f32::consts::TAU);
    let scaled = angle * count as f32 / std::f32::consts::TAU;
    let sector = scaled.floor() as usize % count;
    let start = points[sector];
    let end = points[(sector + 1) % count];
    let direction = position.normalize();
    let edge = end - start;
    let boundary_radius = cross(start, edge) / cross(direction, edge);
    let inside = position.length() <= boundary_radius;

    let mut distance = f32::INFINITY;
    for offset in -2..=2 {
        let index = (sector as isize + offset).rem_euclid(count as isize) as usize;
        distance = distance.min(point_segment_distance(
            position,
            points[index],
            points[(index + 1) % count],
        ));
    }
    (inside, distance)
}

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

pub fn point_in_polygon(point: Vec2, polygon: &[Vec2]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &current in polygon {
        if (current.y > point.y) != (previous.y > point.y) {
            let x = (previous.x - current.x) * (point.y - current.y) / (previous.y - current.y)
                + current.x;
            if point.x < x {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

pub fn point_segment_distance(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        (point - a).dot(ab) / ab.length_squared()
    } else {
        0.0
    };
    point.distance(a + ab * t.clamp(0.0, 1.0))
}

fn radial_points(radii: &[f32]) -> Vec<Vec2> {
    radii
        .iter()
        .enumerate()
        .map(|(i, r)| Vec2::from_angle(std::f32::consts::TAU * i as f32 / radii.len() as f32) * *r)
        .collect()
}
fn polygon_area(points: &[Vec2]) -> f32 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a.perp_dot(*b))
        .sum::<f32>()
        * 0.5
}

pub fn choose_initial_spawns(board: &BoardGrid, count: usize, margin: f32, seed: u64) -> Vec<Vec2> {
    let candidates: Vec<Vec2> = (0..board.len())
        .filter(|&i| board.field_mask[i] && board.signed_distance[i] >= margin)
        .map(|i| board.cell_center(board.cell(i)))
        .collect();
    if candidates.is_empty() {
        return vec![Vec2::ZERO; count];
    }
    let mut rng = DeterministicRng::new(seed ^ 0x5350_4157_4e53);
    let mut chosen = vec![candidates[rng.index(candidates.len())]];
    while chosen.len() < count {
        let point = candidates
            .iter()
            .copied()
            .max_by(|a, b| {
                let da = chosen
                    .iter()
                    .map(|p| p.distance_squared(*a))
                    .fold(f32::INFINITY, f32::min);
                let db = chosen
                    .iter()
                    .map(|p| p.distance_squared(*b))
                    .fold(f32::INFINITY, f32::min);
                da.total_cmp(&db)
                    .then_with(|| a.x.total_cmp(&b.x))
                    .then_with(|| a.y.total_cmp(&b.y))
            })
            .unwrap();
        chosen.push(point);
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coordinates_round_trip() {
        let b = BoardGrid::generate(4, 2, &GameConfig::default());
        let c = Cell::new(12, 21);
        assert_eq!(b.world_to_cell(b.cell_center(c)), Some(c));
    }
    #[test]
    fn negative_coordinates_are_rejected_without_eager_index_arithmetic() {
        let board = BoardGrid::generate(4, 2, &GameConfig::default());
        assert_eq!(board.index(Cell::new(-1, 0)), None);
        assert_eq!(board.index(Cell::new(0, -1)), None);
    }
    #[test]
    fn fields_are_connected_and_deterministic() {
        for seed in 0..8 {
            let a = BoardGrid::generate(seed, 8, &GameConfig::default());
            let b = BoardGrid::generate(seed, 8, &GameConfig::default());
            assert_eq!(a.field_mask, b.field_mask);
            assert!(a.connected_playable());
            assert!(a.playable_cells > 16_000);
        }
    }

    #[test]
    fn radial_sampling_matches_polygon_classification() {
        let board = BoardGrid::generate(91, 12, &GameConfig::default());
        for index in 0..board.len() {
            let position = board.cell_center(board.cell(index));
            assert_eq!(
                board.field_mask[index],
                point_in_polygon(position, &board.contour.points),
                "classification differs at {position:?}"
            );
        }
    }
    #[test]
    fn initial_seeds_do_not_overlap() {
        let cfg = GameConfig::default();
        let b = BoardGrid::generate(1, 12, &cfg);
        let s = choose_initial_spawns(&b, 12, 8.0, 1);
        for i in 0..s.len() {
            for j in 0..i {
                assert!(s[i].distance(s[j]) > cfg.starting_territory_radius * 2.0);
            }
        }
    }
    #[test]
    fn inward_direction_increases_distance() {
        let b = BoardGrid::generate(2, 2, &GameConfig::default());
        let p = b.contour.points[0] - Vec2::X;
        let n = b.inward_normal(p);
        assert!(
            b.signed_distance_at(p + n * b.cell_size) >= b.signed_distance_at(p - n * b.cell_size)
        );
    }

    #[test]
    fn nearby_trail_query_deduplicates_bucket_references() {
        let mut board = BoardGrid::generate(23, 2, &GameConfig::default());
        let center = board.world_to_cell(Vec2::ZERO).unwrap();
        let reference = TrailSegmentRef {
            owner: CompetitorId(1),
            segment: 3,
        };
        for cell in [center, Cell::new(center.x + 1, center.y)] {
            let index = board.index(cell).unwrap();
            board.trail_segment_buckets[index].push(reference);
        }
        let mut found = Vec::new();
        board.collect_nearby_trail_segments(Vec2::ZERO, 2.0, &mut found);
        assert_eq!(found, vec![reference]);
        board.collect_nearby_trail_segments(Vec2::splat(20.0), 1.0, &mut found);
        assert!(found.is_empty());
    }
}
