use bevy::prelude::*;

use crate::{board::BoardGrid, config::GameConfig};

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct CompetitorMotion {
    pub position: Vec2,
    pub previous_position: Vec2,
    pub heading: Vec2,
}

impl CompetitorMotion {
    pub fn new(position: Vec2, heading: Vec2) -> Self {
        Self {
            position,
            previous_position: position,
            heading: heading.try_normalize().unwrap_or(Vec2::Y),
        }
    }
}

/// Pure authoritative movement step, useful for replay and tests as well as ECS.
pub fn advance_motion(
    motion: &mut CompetitorMotion,
    desired: Option<Vec2>,
    board: &BoardGrid,
    config: &GameConfig,
    dt: f32,
) {
    motion.previous_position = motion.position;
    if let Some(desired) = desired.and_then(Vec2::try_normalize) {
        let cross = motion.heading.perp_dot(desired);
        let dot = motion.heading.dot(desired).clamp(-1.0, 1.0);
        let delta = cross.atan2(dot).clamp(
            -config.max_turn_rate_radians * dt,
            config.max_turn_rate_radians * dt,
        );
        let (sin, cos) = delta.sin_cos();
        motion.heading = Vec2::new(
            motion.heading.x * cos - motion.heading.y * sin,
            motion.heading.x * sin + motion.heading.y * cos,
        )
        .normalize_or_zero();
    }
    let attempted = motion.position + motion.heading * config.player_speed * dt;
    if board.signed_distance_at(attempted) >= config.collision_radius {
        motion.position = attempted;
        return;
    }
    let safe = if board.signed_distance_at(motion.position) >= config.collision_radius {
        motion.position
    } else {
        board.nearest_interior(attempted, config.collision_radius)
    };
    let inward = board.inward_normal(safe);
    let outward_component = motion.heading.dot(-inward).max(0.0);
    let tangent = (motion.heading + inward * outward_component).try_normalize();
    let fallback_a = Vec2::new(-inward.y, inward.x);
    let fallback_b = -fallback_a;
    let tangent = tangent.unwrap_or_else(|| {
        if fallback_a.dot(motion.heading) >= fallback_b.dot(motion.heading) {
            fallback_a
        } else {
            fallback_b
        }
    });
    motion.heading = (tangent + inward * config.inward_edge_steer).normalize_or_zero();
    let slid = safe + motion.heading * config.player_speed * dt;
    motion.position = if board.signed_distance_at(slid) >= config.collision_radius {
        slid
    } else {
        safe
    };
}

pub fn segment_cell_entry_time(
    board: &BoardGrid,
    from: Vec2,
    to: Vec2,
    predicate: impl Fn(crate::board::Cell) -> bool,
) -> Option<f32> {
    // A fixed iteration binary search follows a coarse deterministic bracket and is enough to
    // chronologically order captures within a 1/60 second update.
    const STEPS: usize = 16;
    let mut previous_t = 0.0;
    let mut previous = predicate(board.world_to_cell(from)?);
    for step in 1..=STEPS {
        let t = step as f32 / STEPS as f32;
        let now = predicate(board.world_to_cell(from.lerp(to, t))?);
        if now && !previous {
            let mut lo = previous_t;
            let mut hi = t;
            for _ in 0..12 {
                let mid = (lo + hi) * 0.5;
                if predicate(board.world_to_cell(from.lerp(to, mid))?) {
                    hi = mid
                } else {
                    lo = mid
                }
            }
            return Some(hi);
        }
        previous = now;
        previous_t = t;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn turn_rate_prevents_instant_reversal() {
        let cfg = GameConfig::default();
        let b = BoardGrid::generate(1, 2, &cfg);
        let mut m = CompetitorMotion::new(Vec2::ZERO, Vec2::X);
        advance_motion(&mut m, Some(-Vec2::X), &b, &cfg, 1.0 / 60.0);
        assert!(m.heading.dot(Vec2::X) > 0.99);
    }
    #[test]
    fn edge_is_nonlethal_and_slides() {
        let cfg = GameConfig::default();
        let b = BoardGrid::generate(3, 2, &cfg);
        let edge = b.contour.points[0];
        let mut m = CompetitorMotion::new(edge - Vec2::X, Vec2::X);
        advance_motion(&mut m, None, &b, &cfg, 1.0);
        assert!(b.signed_distance_at(m.position) >= cfg.collision_radius - 0.51);
        assert!(m.heading.dot(Vec2::X) < 0.99);
    }
}
