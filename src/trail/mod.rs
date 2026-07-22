use bevy::prelude::*;

use crate::{
    board::{BoardGrid, Cell, TrailSegmentRef, point_segment_distance},
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
    /// Exact current head retained separately from the sampled polyline.
    pub head: Vec2,
    /// Last head included in the incremental raster update.
    pub rasterized_head: Vec2,
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
            head: boundary,
            rasterized_head: boundary,
        }
    }

    pub fn append(
        &mut self,
        point: Vec2,
        heading: Vec2,
        distance_threshold: f32,
        angle_threshold: f32,
    ) -> bool {
        let moved = self.head.distance(point);
        self.length += moved;
        self.head = point;
        let last_sample = self.points.last().copied().unwrap_or(point);
        let moved_since_sample = last_sample.distance(point);
        let turned = self.last_sample_heading.angle_to(heading).abs();
        if moved_since_sample + 1e-5 < distance_threshold && turned + 1e-5 < angle_threshold {
            return false;
        }
        self.points.push(point);
        self.last_sample_heading = heading;
        true
    }

    pub fn append_exact(&mut self, point: Vec2) {
        if self.head.distance_squared(point) > 1e-8 {
            self.length += self.head.distance(point);
            self.head = point;
        }
        if self
            .points
            .last()
            .is_none_or(|last| last.distance_squared(point) > 1e-8)
        {
            self.points.push(point);
        }
    }

    pub fn segment_count(&self) -> usize {
        self.points.len().saturating_sub(1)
            + usize::from(
                self.points
                    .last()
                    .is_some_and(|point| point.distance_squared(self.head) > 1e-8),
            )
    }

    pub fn segment(&self, index: usize) -> Option<(Vec2, Vec2)> {
        if index + 1 < self.points.len() {
            return Some((self.points[index], self.points[index + 1]));
        }
        (index == self.points.len().saturating_sub(1))
            .then(|| self.points.last().copied().zip(Some(self.head)))
            .flatten()
            .filter(|(start, end)| start.distance_squared(*end) > 1e-8)
    }

    /// A bounded, render-only representation. The exact authoritative head
    /// is included, while long history is deterministically decimated so mesh
    /// size and per-frame work cannot grow with match duration.
    pub fn render_points(&self, maximum: usize) -> Vec<Vec2> {
        let maximum = maximum.max(2);
        let has_exact_head = self
            .points
            .last()
            .is_none_or(|point| point.distance_squared(self.head) > 1e-8);
        let source_len = self.points.len() + usize::from(has_exact_head);
        if source_len <= maximum {
            let mut source = self.points.clone();
            if has_exact_head {
                source.push(self.head);
            }
            return source;
        }
        (0..maximum)
            .map(|index| {
                let source_index = index * (source_len - 1) / (maximum - 1);
                if source_index < self.points.len() {
                    self.points[source_index]
                } else {
                    self.head
                }
            })
            .collect()
    }
}

pub fn clear_trail_bits(board: &mut BoardGrid, player: CompetitorId, cells: &[usize]) {
    let bit = !(1u16 << player.index());
    for &index in cells {
        if index < board.active_trail_bits.len() {
            board.active_trail_bits[index] &= bit;
            board.trail_segment_buckets[index].retain(|reference| reference.owner != player);
        }
    }
}

pub fn rasterize_trail(board: &BoardGrid, points: &[Vec2], width: f32) -> Vec<usize> {
    let mut result = Vec::new();
    for segment in points.windows(2) {
        rasterize_capsule(board, segment[0], segment[1], width * 0.5, &mut result);
    }
    if points.len() == 1 {
        rasterize_capsule(board, points[0], points[0], width * 0.5, &mut result);
    }
    result.sort_unstable();
    result.dedup();
    result
}

pub fn update_trail_raster(board: &mut BoardGrid, trail: &mut ActiveTrail, width: f32) {
    let segment_index = match trail.points.len() {
        0 | 1 => 0,
        length if trail.points[length - 1].distance_squared(trail.head) <= 1e-8 => length - 2,
        length => length - 1,
    };
    let start = trail.rasterized_head;
    let end = trail.head;
    rasterize_increment(
        board,
        start,
        end,
        width * 0.5,
        trail.owner,
        segment_index,
        &mut trail.cells,
    );
    trail.rasterized_head = end;
}

fn rasterize_capsule(board: &BoardGrid, a: Vec2, b: Vec2, radius: f32, result: &mut Vec<usize>) {
    // Including half a cell diagonal makes the discrete mask conservative and prevents diagonal gaps.
    let coverage = radius + board.cell_size * std::f32::consts::FRAC_1_SQRT_2;
    let min = a.min(b) - Vec2::splat(coverage);
    let max = a.max(b) + Vec2::splat(coverage);
    let Some((min_cell, max_cell)) = board.clamped_cell_bounds(min, max) else {
        return;
    };
    for y in min_cell.y..=max_cell.y {
        for x in min_cell.x..=max_cell.x {
            let cell = Cell::new(x, y);
            if let Some(index) = board.index(cell)
                && board.field_mask[index]
                && point_segment_distance(board.cell_center(cell), a, b) <= coverage
            {
                result.push(index);
            }
        }
    }
}

fn rasterize_increment(
    board: &mut BoardGrid,
    a: Vec2,
    b: Vec2,
    radius: f32,
    owner: CompetitorId,
    segment: usize,
    cells: &mut Vec<usize>,
) {
    let coverage = radius + board.cell_size * std::f32::consts::FRAC_1_SQRT_2;
    let Some((min_cell, max_cell)) = board.clamped_cell_bounds(
        a.min(b) - Vec2::splat(coverage),
        a.max(b) + Vec2::splat(coverage),
    ) else {
        return;
    };
    let bit = 1u16 << owner.index();
    let reference = TrailSegmentRef { owner, segment };
    for y in min_cell.y..=max_cell.y {
        for x in min_cell.x..=max_cell.x {
            let cell = Cell::new(x, y);
            let Some(index) = board.index(cell) else {
                continue;
            };
            if !board.field_mask[index]
                || point_segment_distance(board.cell_center(cell), a, b) > coverage
            {
                continue;
            }
            if board.active_trail_bits[index] & bit == 0 {
                board.active_trail_bits[index] |= bit;
                cells.push(index);
            }
            let bucket = &mut board.trail_segment_buckets[index];
            if !bucket.contains(&reference) {
                bucket.push(reference);
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

pub fn swept_active_trail_impact(
    previous: Vec2,
    current: Vec2,
    radius: f32,
    trail: &ActiveTrail,
    candidates: &[usize],
) -> Option<f32> {
    candidates
        .iter()
        .filter_map(|&index| {
            trail
                .segment(index)
                .and_then(|(a, b)| swept_point_capsule_t(previous, current, a, b, radius))
        })
        .min_by(f32::total_cmp)
}

pub fn swept_self_active_trail_impact(
    previous: Vec2,
    current: Vec2,
    radius: f32,
    trail: &ActiveTrail,
    excluded_distance: f32,
    candidates: &[usize],
) -> Option<f32> {
    let count = trail.segment_count();
    if count == 0 {
        return None;
    }
    let mut excluded = excluded_distance;
    let mut cutoff_segment = None;
    let mut cutoff = Vec2::ZERO;
    for index in (0..count).rev() {
        let Some((start, end)) = trail.segment(index) else {
            continue;
        };
        let length = start.distance(end);
        if excluded + 1e-8 >= length {
            excluded -= length;
            continue;
        }
        cutoff_segment = Some(index);
        cutoff = end.lerp(start, (excluded / length).clamp(0.0, 1.0));
        break;
    }
    let cutoff_segment = cutoff_segment?;
    candidates
        .iter()
        .filter_map(|&index| {
            let (start, mut end) = trail.segment(index)?;
            if index > cutoff_segment {
                return None;
            }
            if index == cutoff_segment {
                end = cutoff;
                if start.distance_squared(end) <= 1e-8 {
                    return None;
                }
            }
            swept_point_capsule_t(previous, current, start, end, radius)
        })
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

    #[test]
    fn near_zero_exact_head_does_not_create_missing_segment() {
        let mut trail = ActiveTrail::new(CompetitorId(0), Cell::new(0, 0), Vec2::ZERO, Vec2::X);
        trail.head = Vec2::new(1e-5, 0.0);
        assert_eq!(trail.segment_count(), 0);
        assert!(trail.segment(0).is_none());
    }

    #[test]
    fn sampling_keeps_exact_head_without_unbounded_fixed_tick_history() {
        let mut trail = ActiveTrail::new(CompetitorId(0), Cell::new(0, 0), Vec2::ZERO, Vec2::X);
        for step in 1..=100 {
            trail.append(
                Vec2::new(step as f32 * 0.1, 0.0),
                Vec2::X,
                0.2,
                6.0_f32.to_radians(),
            );
        }
        assert_eq!(trail.head, Vec2::new(10.0, 0.0));
        assert!((trail.length - 10.0).abs() < 1e-4);
        assert!(trail.points.len() < 60);
    }

    #[test]
    fn incremental_raster_matches_the_conservative_full_mask() {
        let config = GameConfig::default();
        let mut board = BoardGrid::generate(44, 2, &config);
        let owner = CompetitorId(0);
        let start = Vec2::new(-4.0, -4.0);
        let mut trail =
            ActiveTrail::new(owner, board.world_to_cell(start).unwrap(), start, Vec2::X);
        for step in 1..=80 {
            let point = Vec2::new(-4.0 + step as f32 * 0.1, -4.0 + step as f32 * 0.06);
            trail.append(point, Vec2::X, 0.2, config.trail_sample_angle_radians);
            update_trail_raster(&mut board, &mut trail, config.trail_width);
        }
        let mut full_points = trail.points.clone();
        if full_points.last() != Some(&trail.head) {
            full_points.push(trail.head);
        }
        let mut expected = rasterize_trail(&board, &full_points, config.trail_width);
        let mut actual = trail.cells.clone();
        expected.sort_unstable();
        actual.sort_unstable();
        assert_eq!(actual, expected);
    }

    #[test]
    fn render_history_has_a_hard_point_budget() {
        let mut trail = ActiveTrail::new(CompetitorId(0), Cell::new(0, 0), Vec2::ZERO, Vec2::X);
        for step in 1..=2_000 {
            trail.append_exact(Vec2::new(step as f32 * 0.1, 0.0));
        }
        let rendered = trail.render_points(64);
        assert!(rendered.len() <= 64);
        assert_eq!(rendered.first(), Some(&Vec2::ZERO));
        assert_eq!(rendered.last(), Some(&trail.head));
    }
}
