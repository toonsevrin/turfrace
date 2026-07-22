use std::{cmp::Ordering, collections::BinaryHeap};

use bevy::prelude::*;

use crate::{
    board::{BoardGrid, Cell, point_in_polygon},
    ids::CompetitorId,
    trail::ActiveTrail,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptureResult {
    pub claimed_cells: Vec<usize>,
    pub stolen_by_owner: Vec<(CompetitorId, u32)>,
    pub used_loop_fill: bool,
}

#[derive(Clone, Copy)]
struct OpenNode {
    index: usize,
    estimate: f32,
    cost: f32,
}
impl PartialEq for OpenNode {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.estimate.to_bits() == other.estimate.to_bits()
    }
}
impl Eq for OpenNode {}
impl PartialOrd for OpenNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OpenNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimate
            .total_cmp(&self.estimate)
            .then_with(|| other.cost.total_cmp(&self.cost))
            .then_with(|| other.index.cmp(&self.index))
    }
}

/// Deterministic eight-way A* through current ownership, with no diagonal corner cutting.
pub fn find_owned_path(
    board: &BoardGrid,
    player: CompetitorId,
    start: Cell,
    goal: Cell,
) -> Option<Vec<Cell>> {
    let start_index = board.index(start)?;
    let goal_index = board.index(goal)?;
    if !board.owns(start, player) || !board.owns(goal, player) {
        return None;
    }
    let mut cost = vec![f32::INFINITY; board.len()];
    let mut parent = vec![usize::MAX; board.len()];
    let mut open = BinaryHeap::new();
    cost[start_index] = 0.0;
    open.push(OpenNode {
        index: start_index,
        cost: 0.0,
        estimate: octile(start, goal),
    });
    while let Some(node) = open.pop() {
        if node.index == goal_index {
            let mut path = vec![goal];
            let mut at = goal_index;
            while at != start_index {
                at = parent[at];
                path.push(board.cell(at));
            }
            path.reverse();
            return Some(path);
        }
        if node.cost > cost[node.index] + 1e-5 {
            continue;
        }
        let cell = board.cell(node.index);
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let next = Cell::new(cell.x + dx, cell.y + dy);
                let Some(next_index) = board.index(next) else {
                    continue;
                };
                if !board.owns(next, player) {
                    continue;
                }
                if dx != 0
                    && dy != 0
                    && (!board.owns(Cell::new(cell.x + dx, cell.y), player)
                        || !board.owns(Cell::new(cell.x, cell.y + dy), player))
                {
                    continue;
                }
                let next_cost = node.cost
                    + if dx == 0 || dy == 0 {
                        1.0
                    } else {
                        std::f32::consts::SQRT_2
                    };
                if next_cost + 1e-5 < cost[next_index] {
                    cost[next_index] = next_cost;
                    parent[next_index] = node.index;
                    open.push(OpenNode {
                        index: next_index,
                        cost: next_cost,
                        estimate: next_cost + octile(next, goal),
                    });
                }
            }
        }
    }
    None
}

fn octile(a: Cell, b: Cell) -> f32 {
    let dx = (a.x - b.x).unsigned_abs() as f32;
    let dy = (a.y - b.y).unsigned_abs() as f32;
    dx.max(dy) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dy)
}

pub fn simplify_cell_path(board: &BoardGrid, path: &[Cell]) -> Vec<Vec2> {
    if path.len() < 3 {
        return path.iter().map(|&c| board.cell_center(c)).collect();
    }
    let mut result = vec![board.cell_center(path[0])];
    let mut previous = (path[1].x - path[0].x, path[1].y - path[0].y);
    for i in 1..path.len() - 1 {
        let next = (path[i + 1].x - path[i].x, path[i + 1].y - path[i].y);
        if next != previous {
            result.push(board.cell_center(path[i]));
            previous = next;
        }
    }
    result.push(board.cell_center(*path.last().unwrap()));
    result
}

pub fn polygon_fill_cells(board: &BoardGrid, polygon: &[Vec2]) -> Vec<usize> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    let min = polygon.iter().copied().reduce(Vec2::min).unwrap();
    let max = polygon.iter().copied().reduce(Vec2::max).unwrap();
    let Some((a, b)) = board.clamped_cell_bounds(min, max) else {
        return Vec::new();
    };
    let mut cells = Vec::new();
    for y in a.y..=b.y {
        for x in a.x..=b.x {
            let c = Cell::new(x, y);
            if let Some(i) = board.index(c)
                && board.field_mask[i]
                && point_in_polygon(board.cell_center(c), polygon)
            {
                cells.push(i)
            }
        }
    }
    cells
}

/// Calculate a closure from a snapshot. Applying it is kept separate for equal-time conflict resolution.
pub fn calculate_capture(
    board: &BoardGrid,
    player: CompetitorId,
    trail: &ActiveTrail,
    end: Cell,
) -> CaptureResult {
    let mut claimed: Vec<usize> = trail
        .cells
        .iter()
        .copied()
        .filter(|&i| board.field_mask[i])
        .collect();
    claimed.sort_unstable();
    claimed.dedup();
    let mut used_loop_fill = false;
    if let Some(path) = find_owned_path(board, player, trail.start_owned_cell, end) {
        let mut polygon = trail.points.clone();
        if polygon
            .last()
            .is_none_or(|point| point.distance_squared(trail.head) > 1e-8)
        {
            polygon.push(trail.head);
        }
        let mut owned = simplify_cell_path(board, &path);
        owned.reverse();
        polygon.extend(owned);
        claimed.extend(polygon_fill_cells(board, &polygon));
        claimed.sort_unstable();
        claimed.dedup();
        used_loop_fill = true;
    }
    CaptureResult {
        claimed_cells: claimed.into_iter().collect(),
        stolen_by_owner: Vec::new(),
        used_loop_fill,
    }
}

pub fn apply_capture(board: &mut BoardGrid, player: CompetitorId, result: &mut CaptureResult) {
    let mut stolen = [0u32; crate::ids::MAX_COMPETITORS];
    for &index in &result.claimed_cells {
        if let Some(previous) = board.owner[index].competitor()
            && previous != player
        {
            stolen[previous.index()] += 1;
        }
        board.set_owner_index(index, player.owner());
    }
    result.stolen_by_owner = stolen
        .into_iter()
        .enumerate()
        .filter(|(_, n)| *n > 0)
        .map(|(i, n)| (CompetitorId(i as u8), n))
        .collect();
}

pub fn apply_equal_time_captures(
    board: &mut BoardGrid,
    captures: &mut [(CompetitorId, CaptureResult)],
) {
    // Snapshot calculations are already complete. Stable ID wins every contested cell.
    captures.sort_by_key(|(id, _)| *id);
    let mut claimed = vec![false; board.len()];
    for (id, result) in captures {
        result.claimed_cells.retain(|cell| {
            !claimed[*cell] && {
                claimed[*cell] = true;
                true
            }
        });
        apply_capture(board, *id, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GameConfig;
    #[test]
    fn a_star_respects_owned_cells_and_corner_rules() {
        let mut b = BoardGrid::generate(1, 2, &GameConfig::default());
        let p = CompetitorId(0);
        for x in 100..105 {
            b.set_owner(Cell::new(x, 100), p.owner());
        }
        let path = find_owned_path(&b, p, Cell::new(100, 100), Cell::new(104, 100)).unwrap();
        assert_eq!(path.len(), 5);
    }
    #[test]
    fn scanline_fills_cell_centers() {
        let b = BoardGrid::generate(2, 2, &GameConfig::default());
        let polygon = [
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ];
        assert_eq!(polygon_fill_cells(&b, &polygon).len(), 16);
    }
    #[test]
    fn bridge_capture_claims_only_corridor() {
        let mut b = BoardGrid::generate(3, 2, &GameConfig::default());
        let p = CompetitorId(0);
        let a = b.world_to_cell(Vec2::new(-4.0, 0.0)).unwrap();
        let z = b.world_to_cell(Vec2::new(4.0, 0.0)).unwrap();
        b.set_owner(a, p.owner());
        b.set_owner(z, p.owner());
        let mut trail = ActiveTrail::new(p, a, b.cell_center(a), Vec2::X);
        trail.append_exact(b.cell_center(z));
        trail.cells = crate::trail::rasterize_trail(&b, &trail.points, 0.65);
        let result = calculate_capture(&b, p, &trail, z);
        assert!(!result.used_loop_fill);
        assert_eq!(result.claimed_cells, trail.cells);
    }

    #[test]
    fn loop_capture_fills_enclosed_cells_deterministically() {
        let mut board = BoardGrid::generate(33, 2, &GameConfig::default());
        let player = CompetitorId(0);
        let start = board.world_to_cell(Vec2::new(-2.0, -2.0)).unwrap();
        let end = board.world_to_cell(Vec2::new(2.0, -2.0)).unwrap();
        for x in start.x..=end.x {
            board.set_owner(Cell::new(x, start.y), player.owner());
        }
        let start_position = board.cell_center(start);
        let end_position = board.cell_center(end);
        let mut trail = ActiveTrail::new(player, start, start_position, Vec2::Y);
        trail.append_exact(Vec2::new(start_position.x, 2.0));
        trail.append_exact(Vec2::new(end_position.x, 2.0));
        trail.append_exact(end_position);
        trail.cells = crate::trail::rasterize_trail(&board, &trail.points, 0.65);

        let first = calculate_capture(&board, player, &trail, end);
        let second = calculate_capture(&board, player, &trail, end);
        assert!(first.used_loop_fill);
        assert_eq!(first, second);
        let center = board
            .index(board.world_to_cell(Vec2::ZERO).unwrap())
            .unwrap();
        assert!(first.claimed_cells.contains(&center));
        assert!(
            first
                .claimed_cells
                .iter()
                .all(|&index| board.field_mask[index])
        );
    }
    #[test]
    fn stealing_updates_both_counts() {
        let mut b = BoardGrid::generate(4, 2, &GameConfig::default());
        let a = CompetitorId(0);
        let z = CompetitorId(1);
        let c = b.world_to_cell(Vec2::ZERO).unwrap();
        b.set_owner(c, z.owner());
        let mut result = CaptureResult {
            claimed_cells: vec![b.index(c).unwrap()],
            ..default()
        };
        apply_capture(&mut b, a, &mut result);
        assert_eq!(b.owner_counts[a.index()], 1);
        assert_eq!(b.owner_counts[z.index()], 0);
        assert!(b.verify_counts());
    }
}
