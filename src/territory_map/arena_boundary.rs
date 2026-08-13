//! Signed-distance queries and collision projection for the arena contour.

use bevy::prelude::*;

use crate::geometry::MultiPolygon;

const TOLERANCE: f32 = 0.001;
const MAX_ERROR: f32 = 0.01;
const MAX_ITERATIONS: usize = 12;

/// Cohesive geometric view of the arena boundary.
#[derive(Clone, Copy)]
pub struct ArenaBoundary<'a> {
    arena: &'a MultiPolygon,
}

impl<'a> ArenaBoundary<'a> {
    pub(super) const fn new(arena: &'a MultiPolygon) -> Self {
        Self { arena }
    }

    pub fn signed_distance(self, point: Vec2) -> f32 {
        if self.arena.is_empty() {
            -f32::INFINITY
        } else {
            let distance = self.arena.boundary_distance(point);
            if self.arena.contains_world(point) {
                distance
            } else {
                -distance
            }
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
