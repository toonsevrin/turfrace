//! Signed-distance queries and collision projection for the arena contour.

use bevy::prelude::*;

use crate::geometry::{MultiPolygon, point_segment_distance_squared};

const TOLERANCE: f32 = 0.001;
const MAX_ERROR: f32 = 0.01;
const MAX_ITERATIONS: usize = 12;

/// Signed distance is 1-Lipschitz away from the fixed-point containment
/// boundary. Cell-center samples plus a rounding guard therefore answer most
/// interior margin checks without walking the arena contour. Exact geometry
/// remains the fallback near contact, outside the cache, and for invalid input.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ArenaDistanceCache {
    origin: Vec2,
    cell_size: Vec2,
    rounding_guard: f32,
    distances: Vec<f32>,
    segments: Vec<BoundarySegment>,
    candidate_offsets: Vec<usize>,
    candidate_segments: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BoundarySegment {
    start: Vec2,
    end: Vec2,
}

impl ArenaDistanceCache {
    const SIDE: usize = 64;

    fn new_empty() -> Self {
        Self::default()
    }

    pub(super) fn new(arena: &MultiPolygon) -> Self {
        let Some((min, max)) = arena.bounds() else {
            return Self::new_empty();
        };
        let origin = min.world();
        let cell_size = (max.world() - origin) / Self::SIDE as f32;
        if !cell_size.is_finite() || cell_size.min_element() <= 0.0 {
            return Self::new_empty();
        }
        let segments: Vec<_> = arena
            .polygons
            .iter()
            .flat_map(|polygon| std::iter::once(&polygon.outer).chain(polygon.holes.iter()))
            .filter(|contour| contour.len() >= 2)
            .flat_map(|contour| {
                (0..contour.len()).map(|index| BoundarySegment {
                    start: contour[index].world(),
                    end: contour[(index + 1) % contour.len()].world(),
                })
            })
            .collect();
        if segments.is_empty() {
            return Self::new_empty();
        }

        let mut cache = Self {
            origin,
            cell_size,
            rounding_guard: 0.01
                + origin.abs().max(max.world().abs()).max_element() * f32::EPSILON * 64.0,
            distances: Vec::with_capacity(Self::SIDE * Self::SIDE),
            segments,
            candidate_offsets: Vec::with_capacity(Self::SIDE * Self::SIDE + 1),
            candidate_segments: Vec::new(),
        };
        let radius = cell_size.length() * 0.5;
        if !radius.is_finite() {
            return Self::new_empty();
        }
        cache.candidate_offsets.push(0);
        // Reuse this one scratch buffer instead of rebuilding the edge list for
        // every cell. The retained indices are deliberately not capped: every
        // segment within the conservative geometric bound is required.
        let mut center_distances = Vec::with_capacity(cache.segments.len());
        for y in 0..Self::SIDE {
            for x in 0..Self::SIDE {
                let point = origin + (Vec2::new(x as f32, y as f32) + Vec2::splat(0.5)) * cell_size;
                center_distances.clear();
                let mut nearest_squared = f32::INFINITY;
                for segment in &cache.segments {
                    let distance_squared =
                        point_segment_distance_squared(point, segment.start, segment.end);
                    center_distances.push(distance_squared);
                    nearest_squared = nearest_squared.min(distance_squared);
                }
                let distance = nearest_squared.sqrt();
                let threshold = distance + 2.0 * radius + cache.rounding_guard;
                if !distance.is_finite() || !threshold.is_finite() {
                    return Self::new_empty();
                }
                let threshold_squared = threshold * threshold;
                for (index, &distance_squared) in center_distances.iter().enumerate() {
                    if threshold_squared.is_infinite() || distance_squared <= threshold_squared {
                        cache.candidate_segments.push(index);
                    }
                }
                cache.distances.push(if arena.contains_world(point) {
                    distance
                } else {
                    -distance
                });
                cache.candidate_offsets.push(cache.candidate_segments.len());
            }
        }
        cache
    }

    fn cell_index(&self, point: Vec2) -> Option<usize> {
        if self.distances.is_empty() || !point.is_finite() {
            return None;
        }
        let relative = (point - self.origin) / self.cell_size;
        if relative.min_element() < 0.0 || relative.max_element() >= Self::SIDE as f32 {
            return None;
        }
        let x = relative.x as usize;
        let y = relative.y as usize;
        Some(y * Self::SIDE + x)
    }

    fn bounds(&self, point: Vec2) -> Option<(f32, f32)> {
        let index = self.cell_index(point)?;
        let x = index % Self::SIDE;
        let y = index / Self::SIDE;
        let center =
            self.origin + (Vec2::new(x as f32, y as f32) + Vec2::splat(0.5)) * self.cell_size;
        let error = point.distance(center) + self.rounding_guard;
        let distance = self.distances[index];
        Some((distance - error, distance + error))
    }

    fn distance(&self, point: Vec2) -> Option<f32> {
        let index = self.cell_index(point)?;
        let start = *self.candidate_offsets.get(index)?;
        let end = *self.candidate_offsets.get(index + 1)?;
        let mut nearest_squared = f32::INFINITY;
        for &segment_index in self.candidate_segments.get(start..end)? {
            nearest_squared = nearest_squared.min(point_segment_distance_squared(
                point,
                self.segments.get(segment_index)?.start,
                self.segments.get(segment_index)?.end,
            ));
        }
        let distance = nearest_squared.sqrt();
        distance.is_finite().then_some(distance)
    }
}

/// Cohesive geometric view of the arena boundary.
#[derive(Clone, Copy)]
pub struct ArenaBoundary<'a> {
    arena: &'a MultiPolygon,
    cache: &'a ArenaDistanceCache,
}

impl<'a> ArenaBoundary<'a> {
    pub(super) const fn new(arena: &'a MultiPolygon, cache: &'a ArenaDistanceCache) -> Self {
        Self { arena, cache }
    }

    /// A conservative broadphase only: ambiguous comparisons always use the
    /// exact vector distance. The immutable arena cache never supplies a
    /// projected position or an approximate gameplay distance.
    pub fn at_most_margin(self, point: Vec2, margin: f32) -> bool {
        if let Some((lower, upper)) = self.cache.bounds(point) {
            if lower > margin {
                return false;
            }
            if upper <= margin {
                return true;
            }
        }
        self.signed_distance(point) <= margin
    }

    pub fn at_least_margin(self, point: Vec2, margin: f32) -> bool {
        if let Some((lower, upper)) = self.cache.bounds(point) {
            if lower >= margin {
                return true;
            }
            if upper < margin {
                return false;
            }
        }
        self.signed_distance(point) >= margin
    }

    pub fn signed_distance(self, point: Vec2) -> f32 {
        if !point.is_finite() || self.arena.is_empty() {
            return -f32::INFINITY;
        }
        let distance = self
            .cache
            .distance(point)
            .unwrap_or_else(|| self.arena.boundary_distance(point));
        if self.arena.contains_world(point) {
            distance
        } else {
            -distance
        }
    }

    pub fn inward_normal(self, point: Vec2) -> Vec2 {
        let epsilon = 0.25;
        let dx = self.signed_distance(point + Vec2::X * epsilon)
            - self.signed_distance(point - Vec2::X * epsilon);
        let dy = self.signed_distance(point + Vec2::Y * epsilon)
            - self.signed_distance(point - Vec2::Y * epsilon);
        Vec2::new(dx, dy)
            .try_normalize()
            .unwrap_or_else(|| -point.try_normalize().unwrap_or(Vec2::Y))
    }

    /// Finds the last point on a segment that satisfies `margin`.
    pub fn margin_exit_time(self, from: Vec2, to: Vec2, margin: f32) -> Option<f32> {
        if !self.valid_input(from, margin)
            || !to.is_finite()
            || self.signed_distance(from) < margin
            || self.signed_distance(to) >= margin
        {
            return None;
        }
        let mut inside = 0.0;
        let mut outside = 1.0;
        for _ in 0..16 {
            let mid = (inside + outside) * 0.5;
            if self.signed_distance(from.lerp(to, mid)) >= margin {
                inside = mid;
            } else {
                outside = mid;
            }
        }
        Some(inside)
    }

    /// Moves an invalid point just far enough inside to satisfy `margin`.
    pub fn project_inside(self, point: Vec2, margin: f32) -> Option<Vec2> {
        if !self.valid_input(point, margin) {
            return None;
        }
        let mut candidate = point;
        for _ in 0..MAX_ITERATIONS {
            let correction = margin - self.signed_distance(candidate);
            if correction <= 0.0 {
                return Some(candidate);
            }
            candidate += self.inward_normal(candidate) * (correction + TOLERANCE);
        }
        (self.signed_distance(candidate) >= margin).then_some(candidate)
    }

    /// Projects a nearby point onto the requested signed-distance contour.
    pub fn project_to_margin(self, point: Vec2, margin: f32) -> Option<Vec2> {
        if !self.valid_input(point, margin) {
            return None;
        }
        let mut candidate = point;
        for _ in 0..MAX_ITERATIONS {
            let correction = margin - self.signed_distance(candidate);
            if correction.abs() <= TOLERANCE {
                return Some(candidate);
            }
            candidate += self.inward_normal(candidate) * correction;
        }
        ((margin - self.signed_distance(candidate)).abs() <= MAX_ERROR).then_some(candidate)
    }

    fn valid_input(self, point: Vec2, margin: f32) -> bool {
        point.is_finite() && margin.is_finite() && margin >= 0.0 && !self.arena.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{board::BoardGrid, config::GameConfig, territory_map::TerritoryMap};

    #[test]
    fn cached_signed_distances_match_full_boundary_scan() {
        let config = GameConfig::default();
        let arenas = [
            TerritoryMap::from_board(&BoardGrid::generate(1, 12, &config))
                .arena
                .clone(),
            TerritoryMap::from_board(&BoardGrid::generate(42, 12, &config))
                .arena
                .clone(),
            {
                let outer = MultiPolygon::from_outer(&[
                    Vec2::splat(-20.0),
                    Vec2::new(20.0, -20.0),
                    Vec2::splat(20.0),
                    Vec2::new(-20.0, 20.0),
                ]);
                let hole = MultiPolygon::from_outer(&[
                    Vec2::splat(-5.0),
                    Vec2::new(5.0, -5.0),
                    Vec2::splat(5.0),
                    Vec2::new(-5.0, 5.0),
                ]);
                outer.difference(&hole)
            },
        ];
        for arena in arenas {
            check_cached_signed_distances(&arena);
        }
    }

    fn check_cached_signed_distances(arena: &MultiPolygon) {
        let map = TerritoryMap::new(arena.clone());
        let boundary = map.arena_boundary();
        let mut points = vec![Vec2::ZERO, Vec2::new(1.2345, -6.789)];
        let Some((min, max)) = arena.bounds() else {
            return;
        };
        let min = min.world();
        let max = max.world();
        let cell_size = (max - min) / ArenaDistanceCache::SIDE as f32;
        for index in 0..=ArenaDistanceCache::SIDE {
            let x = min.x + index as f32 * cell_size.x;
            let y = min.y + index as f32 * cell_size.y;
            points.extend([
                Vec2::new(x, min.y + cell_size.y * 0.37),
                Vec2::new(min.x + cell_size.x * 0.63, y),
            ]);
        }
        for polygon in &arena.polygons {
            for contour in std::iter::once(&polygon.outer).chain(polygon.holes.iter()) {
                for point in contour {
                    points.extend([point.world(), point.world() + Vec2::splat(0.0001)]);
                }
            }
        }
        points.extend([
            min - Vec2::splat(2.0),
            max + Vec2::splat(2.0),
            Vec2::new(min.x - 2.0, max.y + 2.0),
            Vec2::new(max.x + 2.0, min.y - 2.0),
        ]);

        for point in points {
            let expected = if arena.contains_world(point) {
                arena.boundary_distance(point)
            } else {
                -arena.boundary_distance(point)
            };
            assert_eq!(boundary.signed_distance(point), expected, "point {point:?}");
        }
    }

    #[test]
    fn cached_margin_comparisons_match_exact_contours() {
        for seed in [1, 42, 91] {
            let board = BoardGrid::generate(seed, 12, &GameConfig::default());
            let map = TerritoryMap::from_board(&board);
            check_comparisons(&map);
        }
        let outer = MultiPolygon::from_outer(&[
            Vec2::splat(-20.0),
            Vec2::new(20.0, -20.0),
            Vec2::splat(20.0),
            Vec2::new(-20.0, 20.0),
        ]);
        let hole = MultiPolygon::from_outer(&[
            Vec2::splat(-5.0),
            Vec2::new(5.0, -5.0),
            Vec2::splat(5.0),
            Vec2::new(-5.0, 5.0),
        ]);
        check_comparisons(&TerritoryMap::new(outer.difference(&hole)));
        check_comparisons(&TerritoryMap::new(MultiPolygon::empty()));
    }

    fn check_comparisons(map: &TerritoryMap) {
        let boundary = map.arena_boundary();
        for y in -55..=55 {
            for x in -55..=55 {
                let point = Vec2::new(x as f32 * 1.41, y as f32 * 1.37);
                let exact = boundary.signed_distance(point);
                for margin in [
                    -1.0,
                    0.0,
                    0.52,
                    0.53,
                    3.0,
                    exact,
                    exact - 0.0001,
                    exact + 0.0001,
                ] {
                    assert_eq!(boundary.at_least_margin(point, margin), exact >= margin);
                    assert_eq!(boundary.at_most_margin(point, margin), exact <= margin);
                }
            }
        }
        for point in [Vec2::NAN, Vec2::INFINITY, Vec2::ZERO] {
            let exact = boundary.signed_distance(point);
            for margin in [0.0, f32::NAN, f32::INFINITY] {
                assert_eq!(boundary.at_least_margin(point, margin), exact >= margin);
                assert_eq!(boundary.at_most_margin(point, margin), exact <= margin);
            }
        }
    }
}
