use bevy::prelude::*;

use crate::geometry::{MultiPolygon, Point};
use crate::ids::MAX_COMPETITORS;

use super::containment_index::TerritoryContainmentIndex;

const INDEX_SIDE: usize = 64;
#[cfg(test)]
pub(super) const INDEX_CELL_COUNT: usize = INDEX_SIDE * INDEX_SIDE;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct TerritorySpatialIndex {
    origin: Vec2,
    cell_size: Vec2,
    /// One candidate bit per competitor. A zero mask is a known-empty index
    /// cell, distinct from an unavailable/out-of-bounds index lookup.
    pub(super) cells: Vec<u16>,
    containment: TerritoryContainmentIndex,
}

impl TerritorySpatialIndex {
    pub(super) fn rebuild(
        &mut self,
        arena: &MultiPolygon,
        territories: &[MultiPolygon; MAX_COMPETITORS],
    ) {
        self.containment.rebuild(arena, territories);
        let Some((min, max)) = arena.bounds() else {
            self.origin = Vec2::ZERO;
            self.cell_size = Vec2::ZERO;
            self.cells.clear();
            return;
        };
        let origin = min.world();
        let extent = (max.world() - origin).max(Vec2::splat(1.0));
        self.origin = origin;
        self.cell_size = extent / INDEX_SIDE as f32;
        if self.cells.len() != INDEX_SIDE * INDEX_SIDE {
            self.cells.resize(INDEX_SIDE * INDEX_SIDE, 0);
        }
        self.cells.fill(0);
        for (owner, territory) in territories.iter().enumerate() {
            let Some((min, max)) = territory.bounds() else {
                continue;
            };
            let (a, b) = self.bounds(min.world(), max.world());
            for y in a.1..=b.1 {
                for x in a.0..=b.0 {
                    self.cells[y * INDEX_SIDE + x] |= 1 << owner;
                }
            }
        }
    }

    pub(super) fn contains(&self, territory: usize, point: Point) -> Option<bool> {
        self.containment.contains(territory, point)
    }

    pub(super) fn candidate_mask(&self, point: Vec2) -> Option<u16> {
        if self.cells.is_empty()
            || point.x < self.origin.x
            || point.y < self.origin.y
            || point.x > self.origin.x + self.cell_size.x * INDEX_SIDE as f32
            || point.y > self.origin.y + self.cell_size.y * INDEX_SIDE as f32
        {
            return None;
        }
        let relative = (point - self.origin) / self.cell_size;
        let x = relative.x.floor().clamp(0.0, (INDEX_SIDE - 1) as f32) as usize;
        let y = relative.y.floor().clamp(0.0, (INDEX_SIDE - 1) as f32) as usize;
        Some(self.cells[y * INDEX_SIDE + x])
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
}
