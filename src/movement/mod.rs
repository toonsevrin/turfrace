use bevy::prelude::*;

use crate::{config::GameConfig, territory_map::TerritoryMap};

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
    territory: &TerritoryMap,
    config: &GameConfig,
    speed: f32,
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
    let attempted = motion.position + motion.heading * speed.max(0.0) * dt;
    if territory.arena_signed_distance(attempted) >= config.collision_radius {
        motion.position = attempted;
        return;
    }
    let safe = if territory.arena_signed_distance(motion.position) >= config.collision_radius {
        motion.position
    } else {
        territory.nearest_arena_interior(attempted, config.collision_radius)
    };
    let inward = territory.arena_inward_normal(safe);
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
    let slid = safe + motion.heading * speed.max(0.0) * dt;
    motion.position = if territory.arena_signed_distance(slid) >= config.collision_radius {
        slid
    } else {
        safe
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::BoardGrid;
    #[test]
    fn turn_rate_prevents_instant_reversal() {
        let cfg = GameConfig::default();
        let map = TerritoryMap::from_board(&BoardGrid::generate(1, 2, &cfg));
        let mut m = CompetitorMotion::new(Vec2::ZERO, Vec2::X);
        advance_motion(
            &mut m,
            Some(-Vec2::X),
            &map,
            &cfg,
            cfg.player_speed,
            1.0 / 60.0,
        );
        assert!(m.heading.dot(Vec2::X) > 0.99);
    }
    #[test]
    fn edge_is_nonlethal_and_slides() {
        let cfg = GameConfig::default();
        let b = BoardGrid::generate(3, 2, &cfg);
        let map = TerritoryMap::from_board(&b);
        let edge = b.contour.points[0];
        let mut m = CompetitorMotion::new(edge - Vec2::X, Vec2::X);
        advance_motion(&mut m, None, &map, &cfg, cfg.player_speed, 1.0);
        assert!(map.arena_signed_distance(m.position) >= cfg.collision_radius - 0.51);
        assert!(m.heading.dot(Vec2::X) < 0.99);
    }

    #[test]
    fn arena_movement_stays_inside_vector_boundary() {
        let cfg = GameConfig::default();
        let board = BoardGrid::generate(19, 2, &cfg);
        let map = TerritoryMap::from_board(&board);
        let edge = board.contour.points[0];
        let mut motion = CompetitorMotion::new(edge - Vec2::X, Vec2::X);
        advance_motion(&mut motion, None, &map, &cfg, cfg.player_speed, 1.0);
        assert!(map.arena_signed_distance(motion.position) >= cfg.collision_radius - 0.5);
    }

    #[test]
    fn kill_speed_uses_the_same_boundary_safe_movement_path() {
        let cfg = GameConfig::default();
        let board = BoardGrid::generate(29, 2, &cfg);
        let map = TerritoryMap::from_board(&board);
        let mut base = CompetitorMotion::new(Vec2::ZERO, Vec2::X);
        let mut boosted = CompetitorMotion::new(Vec2::ZERO, Vec2::X);
        advance_motion(&mut base, None, &map, &cfg, cfg.player_speed, 0.1);
        advance_motion(
            &mut boosted,
            None,
            &map,
            &cfg,
            cfg.player_speed_for_kills(4),
            0.1,
        );
        assert!(boosted.position.x > base.position.x);
        assert!(map.arena_signed_distance(boosted.position) >= cfg.collision_radius - 0.01);
    }
}
