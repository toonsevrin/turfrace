use crate::geometry::{Containment, Edge, MultiPolygon, Point, classify_edges};

const BUCKET_COUNT: usize = 64;

#[derive(Clone, Debug, Default, PartialEq)]
struct ContourBuckets {
    buckets: Vec<Vec<Edge>>,
}

impl ContourBuckets {
    fn new() -> Self {
        Self {
            buckets: (0..BUCKET_COUNT).map(|_| Vec::new()).collect(),
        }
    }

    fn add(&mut self, contour: &[Point], min_y: i32, max_y: i32, bucket_size: i64) {
        if contour.len() < 3 {
            return;
        }
        let mut previous = contour[contour.len() - 1];
        for &current in contour {
            let low = previous.y.min(current.y).max(min_y);
            let high = previous.y.max(current.y).min(max_y);
            if low <= high {
                let first = ((i64::from(low) - i64::from(min_y)) / bucket_size)
                    .min((BUCKET_COUNT - 1) as i64) as usize;
                let last = ((i64::from(high) - i64::from(min_y)) / bucket_size)
                    .min((BUCKET_COUNT - 1) as i64) as usize;
                let edge = Edge::new(previous, current);
                for bucket in first..=last {
                    self.buckets[bucket].push(edge);
                }
            }
            previous = current;
        }
    }

    fn classify(&self, bucket: usize, point: Point) -> Containment {
        classify_edges(point, self.buckets[bucket].iter().copied())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct IndexedPolygon {
    outer: ContourBuckets,
    holes: Vec<ContourBuckets>,
}

impl IndexedPolygon {
    fn contains(&self, bucket: usize, point: Point) -> bool {
        if self.outer.classify(bucket, point) == Containment::Outside {
            return false;
        }
        self.holes
            .iter()
            .all(|hole| hole.classify(bucket, point) != Containment::Inside)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct IndexedTerritory {
    polygons: Vec<IndexedPolygon>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct TerritoryContainmentIndex {
    min_y: i32,
    max_y: i32,
    bucket_size: i64,
    territories: Vec<IndexedTerritory>,
}

impl TerritoryContainmentIndex {
    pub(super) fn rebuild(&mut self, arena: &MultiPolygon, territories: &[MultiPolygon]) {
        let Some((minimum, maximum)) = arena.bounds() else {
            self.territories.clear();
            self.bucket_size = 0;
            return;
        };
        self.min_y = minimum.y;
        self.max_y = maximum.y;
        let span = i64::from(maximum.y) - i64::from(minimum.y) + 1;
        self.bucket_size =
            (span / BUCKET_COUNT as i64) + i64::from(span % BUCKET_COUNT as i64 != 0);
        self.bucket_size = self.bucket_size.max(1);
        self.territories.clear();
        self.territories.reserve(territories.len());
        for territory in territories {
            let mut indexed = IndexedTerritory::default();
            indexed.polygons.reserve(territory.polygons.len());
            for polygon in &territory.polygons {
                let mut indexed_polygon = IndexedPolygon {
                    outer: ContourBuckets::new(),
                    holes: (0..polygon.holes.len())
                        .map(|_| ContourBuckets::new())
                        .collect(),
                };
                indexed_polygon
                    .outer
                    .add(&polygon.outer, self.min_y, self.max_y, self.bucket_size);
                for (indexed_hole, hole) in indexed_polygon.holes.iter_mut().zip(&polygon.holes) {
                    indexed_hole.add(hole, self.min_y, self.max_y, self.bucket_size);
                }
                indexed.polygons.push(indexed_polygon);
            }
            self.territories.push(indexed);
        }
    }

    pub(super) fn contains(&self, territory: usize, point: Point) -> Option<bool> {
        let bucket = self.bucket(point.y)?;
        Some(
            self.territories
                .get(territory)?
                .polygons
                .iter()
                .any(|polygon| polygon.contains(bucket, point)),
        )
    }

    fn bucket(&self, y: i32) -> Option<usize> {
        if self.bucket_size == 0 || y < self.min_y || y > self.max_y {
            return None;
        }
        Some(
            ((i64::from(y) - i64::from(self.min_y)) / self.bucket_size)
                .min((BUCKET_COUNT - 1) as i64) as usize,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::GEOMETRY_SCALE;
    use bevy::prelude::Vec2;

    fn rectangle(min: Vec2, max: Vec2) -> MultiPolygon {
        MultiPolygon::from_outer(&[min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)])
    }

    #[test]
    fn bucketed_containment_preserves_holes_and_integer_boundaries() {
        let arena = rectangle(Vec2::new(-20.0, -20.0), Vec2::new(20.0, 20.0));
        let ring = rectangle(Vec2::new(-10.0, -10.0), Vec2::new(10.0, 10.0))
            .difference(&rectangle(Vec2::new(-3.0, -3.0), Vec2::new(3.0, 3.0)));
        let mut index = TerritoryContainmentIndex::default();
        index.rebuild(&arena, std::slice::from_ref(&ring));
        for y in -12..=12 {
            for x in -12..=12 {
                let world = Vec2::new(x as f32, y as f32);
                let point = Point::from_world(world);
                assert_eq!(
                    index.contains(0, point),
                    Some(ring.contains_world(world)),
                    "mismatch at ({x}, {y})"
                );
            }
        }
        for center in [-3, 3, -10, 10] {
            for offset in -1..=1 {
                let point = Point::new(center * GEOMETRY_SCALE + offset, 0);
                assert_eq!(index.contains(0, point), Some(ring.contains(point)));
            }
        }
    }

    #[test]
    fn disjoint_islands_and_default_index_remain_exact() {
        let arena = rectangle(Vec2::new(-20.0, -20.0), Vec2::new(20.0, 20.0));
        let left = rectangle(Vec2::new(-10.0, -2.0), Vec2::new(-4.0, 2.0));
        let right = rectangle(Vec2::new(4.0, -2.0), Vec2::new(10.0, 2.0));
        let islands = left.union(&right);
        let mut index = TerritoryContainmentIndex::default();
        index.rebuild(&arena, std::slice::from_ref(&islands));
        for world in [
            Vec2::new(-10.0, 0.0),
            Vec2::new(-7.0, 0.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(7.0, 0.0),
            Vec2::new(10.0, 0.0),
        ] {
            assert_eq!(
                index.contains(0, Point::from_world(world)),
                Some(islands.contains_world(world))
            );
        }
        assert_eq!(
            TerritoryContainmentIndex::default().contains(0, Point::new(0, 0)),
            None
        );
    }

    #[test]
    fn queries_outside_indexed_y_range_use_fallback() {
        let arena = rectangle(Vec2::ZERO, Vec2::new(2.0, 2.0));
        let territory = rectangle(Vec2::new(-2.0, -2.0), Vec2::new(4.0, 4.0));
        let mut index = TerritoryContainmentIndex::default();
        index.rebuild(&arena, &[territory]);
        assert_eq!(index.contains(0, Point::new(0, -1)), None);
        assert_eq!(index.contains(0, Point::new(-1, 0)), Some(true));
        assert_eq!(index.contains(0, Point::new(1, 1)), Some(true));
    }
}
