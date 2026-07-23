//! Deterministic fixed-point polygon geometry used by the match simulation.
//!
//! Gameplay never makes decisions from the presentation/sample grid.  All
//! territory operations go through [`MultiPolygon`], which stores integer
//! coordinates and delegates boolean work to `i_overlay`.  Quantising at the
//! API boundary keeps captures deterministic across native and WebAssembly
//! builds while still preserving sub-millimetre detail at game scale.

use bevy::prelude::Vec2;
use i_overlay::{
    core::{fill_rule::FillRule, overlay::Overlay, overlay_rule::OverlayRule},
    i_float::int::point::IntPoint,
    i_shape::int::shape::{IntContour, IntShapes},
};

pub const GEOMETRY_SCALE: i32 = 4096;
const SCALE_SQUARED: f32 = (GEOMETRY_SCALE as f32) * (GEOMETRY_SCALE as f32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn from_world(point: Vec2) -> Self {
        Self::new(quantize(point.x), quantize(point.y))
    }

    pub fn world(self) -> Vec2 {
        Vec2::new(
            self.x as f32 / GEOMETRY_SCALE as f32,
            self.y as f32 / GEOMETRY_SCALE as f32,
        )
    }
}

impl From<Point> for IntPoint<i32> {
    fn from(point: Point) -> Self {
        IntPoint::new(point.x, point.y)
    }
}

impl From<IntPoint<i32>> for Point {
    fn from(point: IntPoint<i32>) -> Self {
        Self::new(point.x, point.y)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polygon {
    pub outer: Vec<Point>,
    pub holes: Vec<Vec<Point>>,
}

impl Polygon {
    pub fn new(mut outer: Vec<Point>) -> Option<Self> {
        normalize_contour(&mut outer);
        (outer.len() >= 3 && area2(&outer).unsigned_abs() > 0).then_some(Self {
            outer,
            holes: Vec::new(),
        })
    }

    pub fn with_holes(mut outer: Vec<Point>, mut holes: Vec<Vec<Point>>) -> Option<Self> {
        normalize_contour(&mut outer);
        for hole in &mut holes {
            normalize_contour(hole);
        }
        holes.retain(|hole| hole.len() >= 3 && area2(hole).unsigned_abs() > 0);
        (outer.len() >= 3 && area2(&outer).unsigned_abs() > 0).then_some(Self { outer, holes })
    }

    pub fn area_scaled(&self) -> i64 {
        let outer = area2(&self.outer).unsigned_abs() / 2;
        let holes: u64 = self
            .holes
            .iter()
            .map(|hole| area2(hole).unsigned_abs() / 2)
            .sum();
        outer.saturating_sub(holes).min(i64::MAX as u64) as i64
    }

    pub fn area(&self) -> f32 {
        self.area_scaled() as f32 / SCALE_SQUARED
    }

    pub fn contains_world(&self, point: Vec2) -> bool {
        self.contains(Point::from_world(point))
    }

    pub fn contains(&self, point: Point) -> bool {
        self.outer.contains_point(point)
            && self.holes.iter().all(|hole| !hole.contains_interior(point))
    }

    pub fn bounds(&self) -> Option<(Point, Point)> {
        bounds(
            self.outer
                .iter()
                .chain(self.holes.iter().flat_map(|h| h.iter())),
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MultiPolygon {
    pub polygons: Vec<Polygon>,
}

impl MultiPolygon {
    pub fn empty() -> Self {
        Self {
            polygons: Vec::new(),
        }
    }

    pub fn from_outer(points: &[Vec2]) -> Self {
        Self::from_contour(points.iter().copied().map(Point::from_world).collect())
    }

    pub fn from_contour(points: Vec<Point>) -> Self {
        Polygon::new(points).map_or_else(Self::empty, |polygon| Self {
            polygons: vec![polygon],
        })
    }

    /// Builds a union from many contours in one overlay pass. This is used
    /// for trail capsules so a long trail never performs an O(n^2) chain of
    /// pairwise polygon unions.
    pub fn from_union_contours(contours: Vec<Vec<Point>>) -> Self {
        let contours: Vec<IntContour<i32>> = contours
            .into_iter()
            .filter_map(|mut contour| {
                normalize_contour(&mut contour);
                (contour.len() >= 3).then(|| contour.into_iter().map(Into::into).collect())
            })
            .collect();
        if contours.is_empty() {
            return Self::empty();
        }
        let mut overlay = Overlay::with_contours(&contours, &[]);
        // All generated stroke contours use the same winding. Non-zero fill
        // therefore treats overlaps as a union in one pass instead of the
        // XOR behaviour of an even-odd subject.
        from_int_shapes(overlay.overlay(OverlayRule::Subject, FillRule::NonZero))
    }

    pub fn area_scaled(&self) -> i64 {
        self.polygons.iter().map(Polygon::area_scaled).sum::<i64>()
    }

    pub fn area(&self) -> f32 {
        self.area_scaled() as f32 / SCALE_SQUARED
    }

    pub fn is_empty(&self) -> bool {
        self.polygons.is_empty()
    }

    pub fn contains_world(&self, point: Vec2) -> bool {
        self.contains(Point::from_world(point))
    }

    pub fn contains(&self, point: Point) -> bool {
        self.polygons.iter().any(|polygon| polygon.contains(point))
    }

    pub fn boundary_distance(&self, point: Vec2) -> f32 {
        let mut nearest = f32::INFINITY;
        for polygon in &self.polygons {
            for contour in std::iter::once(&polygon.outer).chain(polygon.holes.iter()) {
                if contour.len() < 2 {
                    continue;
                }
                for index in 0..contour.len() {
                    nearest = nearest.min(point_segment_distance(
                        point,
                        contour[index].world(),
                        contour[(index + 1) % contour.len()].world(),
                    ));
                }
            }
        }
        nearest
    }

    pub fn bounds(&self) -> Option<(Point, Point)> {
        bounds(self.polygons.iter().flat_map(|polygon| {
            polygon
                .outer
                .iter()
                .chain(polygon.holes.iter().flat_map(|hole| hole.iter()))
        }))
    }

    pub fn contours(&self) -> Vec<IntContour<i32>> {
        self.polygons
            .iter()
            .flat_map(|polygon| {
                std::iter::once(polygon.outer.iter().copied().map(Into::into).collect()).chain(
                    polygon
                        .holes
                        .iter()
                        .map(|hole| hole.iter().copied().map(Into::into).collect()),
                )
            })
            .collect()
    }

    pub fn outer_contours_world(&self) -> Vec<Vec<Vec2>> {
        self.polygons
            .iter()
            .map(|polygon| polygon.outer.iter().map(|point| point.world()).collect())
            .collect()
    }

    pub fn union(&self, other: &Self) -> Self {
        self.boolean(other, OverlayRule::Union)
    }

    pub fn difference(&self, other: &Self) -> Self {
        self.boolean(other, OverlayRule::Difference)
    }

    pub fn intersection(&self, other: &Self) -> Self {
        self.boolean(other, OverlayRule::Intersect)
    }

    pub fn xor(&self, other: &Self) -> Self {
        self.boolean(other, OverlayRule::Xor)
    }

    /// Normalises a collection of contours without changing its fill.
    pub fn normalize(&self) -> Self {
        let mut overlay = Overlay::with_contours(&self.contours(), &[]);
        from_int_shapes(overlay.overlay(OverlayRule::Subject, FillRule::EvenOdd))
    }

    fn boolean(&self, other: &Self, rule: OverlayRule) -> Self {
        if self.is_empty() {
            return match rule {
                OverlayRule::Union | OverlayRule::Xor => other.clone(),
                _ => Self::empty(),
            };
        }
        if other.is_empty() {
            return match rule {
                OverlayRule::Difference | OverlayRule::Union | OverlayRule::Xor => self.clone(),
                OverlayRule::Intersect => Self::empty(),
                _ => Self::empty(),
            };
        }
        let subject = self.contours();
        let clip = other.contours();
        let mut overlay = Overlay::with_contours(&subject, &clip);
        from_int_shapes(overlay.overlay(rule, FillRule::EvenOdd))
    }
}

fn from_int_shapes(shapes: IntShapes<i32>) -> MultiPolygon {
    let polygons = shapes
        .into_iter()
        .filter_map(|shape| {
            let mut contours = shape.into_iter();
            let outer = contours.next()?.into_iter().map(Into::into).collect();
            let holes = contours
                .map(|contour| contour.into_iter().map(Into::into).collect())
                .collect();
            Polygon::with_holes(outer, holes)
        })
        .collect();
    MultiPolygon { polygons }
}

fn quantize(value: f32) -> i32 {
    (value * GEOMETRY_SCALE as f32).round() as i32
}

fn area2(points: &[Point]) -> i64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| i64::from(a.x) * i64::from(b.y) - i64::from(a.y) * i64::from(b.x))
        .sum()
}

fn normalize_contour(points: &mut Vec<Point>) {
    if points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    points.dedup();
    if points.len() < 3 {
        return;
    }
    let mut changed = true;
    while changed && points.len() >= 3 {
        changed = false;
        let count = points.len();
        for index in 0..count {
            let previous = points[(index + count - 1) % count];
            let current = points[index];
            let next = points[(index + 1) % count];
            let ab = (
                i64::from(current.x - previous.x),
                i64::from(current.y - previous.y),
            );
            let bc = (i64::from(next.x - current.x), i64::from(next.y - current.y));
            if ab.0 * bc.1 - ab.1 * bc.0 == 0 && (ab.0 * bc.0 + ab.1 * bc.1) >= 0 {
                points.remove(index);
                changed = true;
                break;
            }
        }
    }
}

fn bounds<'a>(points: impl Iterator<Item = &'a Point>) -> Option<(Point, Point)> {
    let mut iter = points;
    let first = *iter.next()?;
    let mut min = first;
    let mut max = first;
    for point in iter {
        min.x = min.x.min(point.x);
        min.y = min.y.min(point.y);
        max.x = max.x.max(point.x);
        max.y = max.y.max(point.y);
    }
    Some((min, max))
}

trait PointContainment {
    fn contains_point(&self, point: Point) -> bool;
    fn contains_interior(&self, point: Point) -> bool;
}

impl PointContainment for [Point] {
    fn contains_point(&self, point: Point) -> bool {
        if self.len() < 3 {
            return false;
        }
        let mut inside = false;
        let mut previous = self[self.len() - 1];
        for &current in self {
            if point_on_segment(point, previous, current) {
                return true;
            }
            if (current.y > point.y) != (previous.y > point.y) {
                let left = i64::from(previous.x - current.x) * i64::from(point.y - current.y)
                    / i64::from(previous.y - current.y)
                    + i64::from(current.x);
                if i64::from(point.x) < left {
                    inside = !inside;
                }
            }
            previous = current;
        }
        inside
    }

    fn contains_interior(&self, point: Point) -> bool {
        self.contains_point(point)
            && (0..self.len())
                .all(|index| !point_on_segment(point, self[index], self[(index + 1) % self.len()]))
    }
}

fn point_on_segment(point: Point, start: Point, end: Point) -> bool {
    let cross = i64::from(point.x - start.x) * i64::from(end.y - start.y)
        - i64::from(point.y - start.y) * i64::from(end.x - start.x);
    if cross != 0 {
        return false;
    }
    let min_x = start.x.min(end.x);
    let max_x = start.x.max(end.x);
    let min_y = start.y.min(end.y);
    let max_y = start.y.max(end.y);
    point.x >= min_x && point.x <= max_x && point.y >= min_y && point.y <= max_y
}

fn point_segment_distance(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let direction = end - start;
    let t = if direction.length_squared() > f32::EPSILON {
        ((point - start).dot(direction) / direction.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    point.distance(start + direction * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(min: Vec2, max: Vec2) -> MultiPolygon {
        MultiPolygon::from_outer(&[
            Vec2::new(min.x, min.y),
            Vec2::new(max.x, min.y),
            Vec2::new(max.x, max.y),
            Vec2::new(min.x, max.y),
        ])
    }

    #[test]
    fn fixed_point_round_trip_is_bounded() {
        let input = Vec2::new(12.34567, -8.76543);
        let output = Point::from_world(input).world();
        assert!(output.distance(input) < 1.0 / GEOMETRY_SCALE as f32);
    }

    #[test]
    fn boolean_union_and_difference_preserve_area() {
        let left = rectangle(Vec2::ZERO, Vec2::new(4.0, 4.0));
        let right = rectangle(Vec2::new(2.0, 0.0), Vec2::new(6.0, 4.0));
        let union = left.union(&right);
        let intersection = left.intersection(&right);
        let difference = left.difference(&right);
        assert!((union.area() - 24.0).abs() < 0.01);
        assert!((intersection.area() - 8.0).abs() < 0.01);
        assert!((difference.area() - 8.0).abs() < 0.01);
        assert!(union.contains_world(Vec2::new(5.0, 2.0)));
        assert!(!difference.contains_world(Vec2::new(3.0, 2.0)));
    }

    #[test]
    fn batched_union_matches_pairwise_union() {
        let left = rectangle(Vec2::ZERO, Vec2::new(4.0, 4.0));
        let right = rectangle(Vec2::new(2.0, 0.0), Vec2::new(6.0, 4.0));
        let batched = MultiPolygon::from_union_contours(
            left.contours()
                .into_iter()
                .chain(right.contours())
                .map(|contour| contour.into_iter().map(Into::into).collect())
                .collect(),
        );
        assert!((batched.area() - left.union(&right).area()).abs() < 0.01);
    }

    #[test]
    fn holes_and_boundary_are_exact() {
        let outer = rectangle(Vec2::ZERO, Vec2::new(10.0, 10.0));
        let hole = rectangle(Vec2::new(2.0, 2.0), Vec2::new(8.0, 8.0));
        let ring = outer.difference(&hole);
        assert!((ring.area() - 64.0).abs() < 0.01);
        assert!(ring.contains_world(Vec2::new(1.0, 1.0)));
        assert!(!ring.contains_world(Vec2::new(5.0, 5.0)));
        assert!(ring.contains_world(Vec2::new(2.0, 5.0)));
    }

    #[test]
    fn normalization_removes_collinear_duplicates() {
        let polygon = MultiPolygon::from_contour(vec![
            Point::new(0, 0),
            Point::new(GEOMETRY_SCALE, 0),
            Point::new(2 * GEOMETRY_SCALE, 0),
            Point::new(2 * GEOMETRY_SCALE, GEOMETRY_SCALE),
            Point::new(0, GEOMETRY_SCALE),
        ])
        .normalize();
        assert_eq!(polygon.polygons[0].outer.len(), 4);
    }
}
