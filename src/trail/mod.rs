use std::collections::BTreeSet;

use bevy::prelude::*;

use crate::{
    board::{BoardGrid, Cell, point_segment_distance},
    ids::CompetitorId,
};

#[derive(Component, Clone, Debug)]
pub struct ActiveTrail {
    pub owner: CompetitorId,
    pub start_owned_cell: Cell,
    pub points: Vec<Vec2>,
    pub cells: Vec<usize>,
    pub length: f32,
    pub last_sample_heading: Vec2,
}

impl ActiveTrail {
    pub fn new(owner: CompetitorId, start_owned_cell: Cell, boundary: Vec2, heading: Vec2) -> Self {
        Self {
            owner,
            start_owned_cell,
            points: vec![boundary],
            cells: Vec::new(),
            length: 0.0,
            last_sample_heading: heading,
        }
    }

    pub fn append(
        &mut self,
        point: Vec2,
        heading: Vec2,
        distance_threshold: f32,
        angle_threshold: f32,
    ) -> bool {
        let Some(&last) = self.points.last() else {
            self.points.push(point);
            return true;
        };
        let moved = last.distance(point);
        let turned = self.last_sample_heading.angle_to(heading).abs();
        if moved + 1e-5 < distance_threshold && turned + 1e-5 < angle_threshold {
            return false;
        }
        self.length += moved;
        self.points.push(point);
        self.last_sample_heading = heading;
        true
    }

    pub fn append_exact(&mut self, point: Vec2) {
        if self
            .points
            .last()
            .is_none_or(|last| last.distance_squared(point) > 1e-8)
        {
            self.length += self.points.last().map_or(0.0, |last| last.distance(point));
            self.points.push(point);
        }
    }
}

pub fn clear_trail_bits(board: &mut BoardGrid, player: CompetitorId, cells: &[usize]) {
    let bit = !(1u16 << player.index());
    for &index in cells {
        if index < board.active_trail_bits.len() {
            board.active_trail_bits[index] &= bit;
        }
    }
}

pub fn rasterize_trail(board: &BoardGrid, points: &[Vec2], width: f32) -> Vec<usize> {
    let mut result = BTreeSet::new();
    for segment in points.windows(2) {
        rasterize_capsule(board, segment[0], segment[1], width * 0.5, &mut result);
    }
    if points.len() == 1 {
        rasterize_capsule(board, points[0], points[0], width * 0.5, &mut result);
    }
    result.into_iter().collect()
}

pub fn update_trail_raster(board: &mut BoardGrid, trail: &mut ActiveTrail, width: f32) {
    clear_trail_bits(board, trail.owner, &trail.cells);
    trail.cells = rasterize_trail(board, &trail.points, width);
    let bit = 1u16 << trail.owner.index();
    for &index in &trail.cells {
        board.active_trail_bits[index] |= bit;
    }
}

fn rasterize_capsule(
    board: &BoardGrid,
    a: Vec2,
    b: Vec2,
    radius: f32,
    result: &mut BTreeSet<usize>,
) {
    // Including half a cell diagonal makes the discrete mask conservative and prevents diagonal gaps.
    let coverage = radius + board.cell_size * std::f32::consts::FRAC_1_SQRT_2;
    let min = a.min(b) - Vec2::splat(coverage);
    let max = a.max(b) + Vec2::splat(coverage);
    let Some(min_cell) = board.world_to_cell(min) else {
        return;
    };
    let Some(max_cell) = board.world_to_cell(max) else {
        return;
    };
    for y in min_cell.y..=max_cell.y {
        for x in min_cell.x..=max_cell.x {
            let cell = Cell::new(x, y);
            if let Some(index) = board.index(cell)
                && board.field_mask[index]
                && point_segment_distance(board.cell_center(cell), a, b) <= coverage
            {
                result.insert(index);
            }
        }
    }
}

/// Earliest normalized impact time for a moving circle against a trail polyline.
pub fn swept_trail_impact(
    previous: Vec2,
    current: Vec2,
    radius: f32,
    trail: &[Vec2],
) -> Option<f32> {
    trail
        .windows(2)
        .filter_map(|s| swept_point_capsule_t(previous, current, s[0], s[1], radius))
        .min_by(f32::total_cmp)
}

/// Self collision ignores the newest `excluded_distance` world units, including a partial segment.
pub fn swept_self_trail_impact(
    previous: Vec2,
    current: Vec2,
    radius: f32,
    trail: &[Vec2],
    excluded_distance: f32,
) -> Option<f32> {
    if trail.len() < 2 {
        return None;
    }
    let mut remaining = excluded_distance;
    let mut end_index = trail.len() - 1;
    let mut cutoff = trail[end_index];
    while end_index > 0 {
        let start = trail[end_index - 1];
        let len = start.distance(cutoff);
        if remaining < len {
            cutoff = cutoff.lerp(start, remaining / len);
            break;
        }
        remaining -= len;
        end_index -= 1;
        cutoff = start;
    }
    let mut best: Option<f32> = None;
    for segment in trail[..end_index].windows(2) {
        if let Some(t) = swept_point_capsule_t(previous, current, segment[0], segment[1], radius) {
            best = Some(best.map_or(t, |old| old.min(t)));
        }
    }
    if end_index > 0
        && cutoff.distance_squared(trail[end_index - 1]) > 1e-8
        && let Some(t) =
            swept_point_capsule_t(previous, current, trail[end_index - 1], cutoff, radius)
    {
        best = Some(best.map_or(t, |old| old.min(t)));
    }
    best
}

fn swept_point_capsule_t(p0: Vec2, p1: Vec2, a: Vec2, b: Vec2, radius: f32) -> Option<f32> {
    let distance = |t: f32| point_segment_distance(p0.lerp(p1, t), a, b);
    if distance(0.0) <= radius {
        return Some(0.0);
    }
    let mut lo = 0.0;
    let mut hi = 1.0;
    for _ in 0..20 {
        let m1 = (2.0 * lo + hi) / 3.0;
        let m2 = (lo + 2.0 * hi) / 3.0;
        if distance(m1) < distance(m2) {
            hi = m2
        } else {
            lo = m1
        }
    }
    let minimum = (lo + hi) * 0.5;
    if distance(minimum) > radius {
        return None;
    }
    lo = 0.0;
    hi = minimum;
    for _ in 0..22 {
        let mid = (lo + hi) * 0.5;
        if distance(mid) <= radius {
            hi = mid
        } else {
            lo = mid
        }
    }
    Some(hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GameConfig;
    #[test]
    fn rasterization_is_connected_across_diagonals() {
        let b = BoardGrid::generate(1, 2, &GameConfig::default());
        let cells = rasterize_trail(&b, &[Vec2::new(-4.0, -4.0), Vec2::new(4.0, 4.0)], 0.65);
        for pair in cells.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
        for step in 0..16 {
            let p = Vec2::splat(-4.0 + step as f32 * 0.5);
            let c = b.world_to_cell(p).unwrap();
            assert!(cells.contains(&b.index(c).unwrap()));
        }
    }
    #[test]
    fn swept_collision_cannot_tunnel() {
        let trail = [Vec2::new(0.0, -4.0), Vec2::new(0.0, 4.0)];
        let t = swept_trail_impact(Vec2::new(-8.0, 0.0), Vec2::new(8.0, 0.0), 0.5, &trail).unwrap();
        assert!((t - 0.46875).abs() < 0.002);
    }
    #[test]
    fn recent_self_segment_is_excluded() {
        let points = [Vec2::ZERO, Vec2::X * 4.0, Vec2::new(4.0, 4.0)];
        assert!(
            swept_self_trail_impact(Vec2::new(3.5, 3.0), Vec2::new(4.5, 3.0), 0.2, &points, 1.5)
                .is_none()
        );
        assert!(
            swept_self_trail_impact(Vec2::new(3.5, 2.6), Vec2::new(4.5, 2.6), 0.05, &points, 1.5,)
                .is_none()
        );
        assert!(
            swept_self_trail_impact(Vec2::new(3.5, 2.4), Vec2::new(4.5, 2.4), 0.05, &points, 1.5,)
                .is_some()
        );
        assert!(
            swept_self_trail_impact(Vec2::new(3.5, 1.0), Vec2::new(4.5, 1.0), 0.2, &points, 1.5)
                .is_some()
        );
    }
}
