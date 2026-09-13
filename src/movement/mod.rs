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
    *motion = predict_motion(*motion, desired, territory, config, speed, dt);
}

/// Predict one authoritative movement step without mutating an ECS component.
/// NPC planning and replay forecasting use this function so finite turns and
/// arena contact cannot drift from the real simulation.
pub fn predict_motion(
    mut motion: CompetitorMotion,
    desired: Option<Vec2>,
    territory: &TerritoryMap,
    config: &GameConfig,
    speed: f32,
    dt: f32,
) -> CompetitorMotion {
    motion.previous_position = motion.position;
    let speed = speed.max(0.0);
    let step = speed * dt;
    let margin = config.collision_radius;
    let arena = territory.arena_boundary();
    let on_boundary = arena.at_most_margin(motion.position, margin + CONTACT_DISTANCE);
    let mut leave_boundary = false;
    if let Some(mut desired) = desired.and_then(Vec2::try_normalize) {
        if on_boundary {
            let inward = arena.inward_normal(motion.position);
            leave_boundary = desired.dot(inward) > CONTACT_RELEASE_DOT;
            if !leave_boundary {
                desired = boundary_tangent(desired, inward, motion.heading);
            }
        }
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

    if on_boundary && !leave_boundary {
        follow_arena_margin(
            &mut motion,
            territory,
            margin,
            step,
            config.inward_edge_steer,
        );
        return motion;
    }

    let attempted = motion.position + motion.heading * step;
    if arena.at_least_margin(attempted, margin) {
        motion.position = attempted;
        return motion;
    }

    let (safe, remaining_step) = if arena.at_least_margin(motion.position, margin) {
        let fraction = arena
            .margin_exit_time(motion.position, attempted, margin)
            .unwrap_or(0.0);
        (
            motion.position.lerp(attempted, fraction),
            step * (1.0 - fraction),
        )
    } else if let Some(position) = arena.project_inside(motion.position, margin) {
        (position, step)
    } else {
        return motion;
    };
    motion.position = safe;
    follow_arena_margin(
        &mut motion,
        territory,
        margin,
        remaining_step,
        config.inward_edge_steer,
    );
    motion
}

/// Forecast a position using the same bounded movement primitive.
pub fn predict_position(
    motion: CompetitorMotion,
    desired: Option<Vec2>,
    territory: &TerritoryMap,
    config: &GameConfig,
    speed: f32,
    horizon: f32,
) -> Vec2 {
    let horizon = horizon.max(0.0);
    if horizon <= f32::EPSILON {
        return motion.position;
    }
    let samples = (horizon / 0.25).ceil().clamp(1.0, 4.0) as usize;
    let dt = horizon / samples as f32;
    let mut forecast = motion;
    for _ in 0..samples {
        forecast = predict_motion(forecast, desired, territory, config, speed, dt);
    }
    forecast.position
}

/// Geometric contact tolerance and an input dead band prevent signed-distance
/// noise from toggling boundary following without adding lifecycle state.
const CONTACT_DISTANCE: f32 = 0.01;
const CONTACT_RELEASE_DOT: f32 = 0.05;

fn follow_arena_margin(
    motion: &mut CompetitorMotion,
    territory: &TerritoryMap,
    margin: f32,
    step: f32,
    inward_steer: f32,
) {
    let arena = territory.arena_boundary();
    let inward = arena.inward_normal(motion.position);
    let tangent = boundary_tangent(motion.heading, inward, motion.heading);
    let candidate = motion.position + tangent * step;
    let Some(position) = arena.project_to_margin(candidate, margin) else {
        return;
    };
    let inward = arena.inward_normal(position);
    let tangent = boundary_tangent(position - motion.position, inward, tangent);
    motion.position = position;
    motion.heading = (tangent + inward * inward_steer.max(0.0))
        .try_normalize()
        .unwrap_or(tangent);
}

fn boundary_tangent(direction: Vec2, inward: Vec2, previous_heading: Vec2) -> Vec2 {
    let tangent = direction - inward * direction.dot(inward).min(0.0);
    tangent.try_normalize().unwrap_or_else(|| {
        let tangent = Vec2::new(-inward.y, inward.x);
        if tangent.dot(previous_heading) >= 0.0 {
            tangent
        } else {
            -tangent
        }
    })
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
    fn held_outward_input_follows_edge_without_alternating_or_stopping() {
        let cfg = GameConfig::default();
        let board = BoardGrid::generate(11, 2, &cfg);
        let map = TerritoryMap::from_board(&board);
        let boundary = board.contour.points[0];
        let inward = map.arena_inward_normal(boundary);
        let tangent = Vec2::new(-inward.y, inward.x);
        let start = map
            .arena_boundary()
            .project_inside(boundary, cfg.collision_radius + 0.01)
            .unwrap();
        let mut motion = CompetitorMotion::new(start, -inward);
        let desired = (tangent - inward * 2.0).normalize();
        let step = cfg.player_speed * cfg.fixed_delta_seconds();

        advance_motion(
            &mut motion,
            None,
            &map,
            &cfg,
            cfg.player_speed,
            cfg.fixed_delta_seconds(),
        );
        assert!(
            map.arena_signed_distance(motion.position) <= cfg.collision_radius + CONTACT_DISTANCE
        );
        for _ in 0..30 {
            advance_motion(
                &mut motion,
                Some(desired),
                &map,
                &cfg,
                cfg.player_speed,
                cfg.fixed_delta_seconds(),
            );
            assert!(
                motion.position.distance(motion.previous_position) > step * 0.8,
                "edge following should not alternate with stopped frames"
            );
            assert!(map.arena_signed_distance(motion.position) >= cfg.collision_radius - 0.001);
        }
    }

    #[test]
    fn side_input_does_not_alternate_heading_while_following_edge() {
        let cfg = GameConfig::default();
        let board = BoardGrid::generate(17, 2, &cfg);
        let map = TerritoryMap::from_board(&board);
        let boundary = board.contour.points[0];
        let inward = map.arena_inward_normal(boundary);
        let tangent = Vec2::new(-inward.y, inward.x);
        let start = map
            .arena_boundary()
            .project_inside(boundary, cfg.collision_radius + 0.01)
            .unwrap();
        let mut motion = CompetitorMotion::new(start, -inward);

        // Establish real boundary contact before applying sideways input.
        advance_motion(
            &mut motion,
            None,
            &map,
            &cfg,
            cfg.player_speed,
            cfg.fixed_delta_seconds(),
        );
        assert!(
            map.arena_signed_distance(motion.position) <= cfg.collision_radius + CONTACT_DISTANCE
        );

        let desired = (tangent - inward * 0.35).normalize();
        let mut previous_turn_sign = 0.0_f32;
        for frame in 0..60 {
            let previous_heading = motion.heading;
            advance_motion(
                &mut motion,
                Some(desired),
                &map,
                &cfg,
                cfg.player_speed,
                cfg.fixed_delta_seconds(),
            );
            let turn = previous_heading.perp_dot(motion.heading);
            assert!(
                turn * previous_turn_sign >= -0.0001,
                "edge steering should not alternate turn direction"
            );
            if frame > 5 {
                assert!(
                    motion.heading.dot(previous_heading) > 0.995,
                    "held side input should settle instead of oscillating"
                );
            }
            assert!(
                (map.arena_signed_distance(motion.position) - cfg.collision_radius).abs() <= 0.01,
                "boundary following should remain on the margin"
            );
            if turn.abs() > 0.0001 {
                previous_turn_sign = turn;
            }
        }
    }

    #[test]
    fn steering_is_not_constrained_before_motion_reaches_edge() {
        let cfg = GameConfig {
            max_turn_rate_radians: f32::INFINITY,
            ..default()
        };
        let board = BoardGrid::generate(13, 2, &cfg);
        let map = TerritoryMap::from_board(&board);
        let boundary = board.contour.points[0];
        let inward = map.arena_inward_normal(boundary);
        let tangent = Vec2::new(-inward.y, inward.x);
        let start = map
            .arena_boundary()
            .project_inside(boundary, cfg.collision_radius + 0.2)
            .unwrap();
        let mut motion = CompetitorMotion::new(start, inward);
        let outward = -inward;

        advance_motion(
            &mut motion,
            Some(outward),
            &map,
            &cfg,
            cfg.player_speed,
            0.01,
        );

        assert!(
            motion.heading.dot(outward) > 0.99,
            "safe outward steering should not be replaced with a tangent"
        );
        assert!(motion.heading.dot(tangent).abs() < 0.01);
        assert!(map.arena_signed_distance(motion.position) >= cfg.collision_radius);
    }

    #[test]
    fn boundary_slide_preserves_inward_steering_correction() {
        let cfg = GameConfig::default();
        let board = BoardGrid::generate(18, 2, &cfg);
        let map = TerritoryMap::from_board(&board);
        let boundary = board.contour.points[0];
        let inward = map.arena_inward_normal(boundary);
        let start = map
            .arena_boundary()
            .project_inside(boundary, cfg.collision_radius)
            .unwrap();
        let mut motion = CompetitorMotion::new(start, -inward);

        advance_motion(
            &mut motion,
            None,
            &map,
            &cfg,
            cfg.player_speed,
            cfg.fixed_delta_seconds(),
        );

        assert!(motion.heading.dot(map.arena_inward_normal(motion.position)) > 0.0);
    }

    #[test]
    fn crossing_step_slides_only_the_unconsumed_distance() {
        let cfg = GameConfig::default();
        let board = BoardGrid::generate(20, 2, &cfg);
        let map = TerritoryMap::from_board(&board);
        let boundary = board.contour.points[0];
        let inward = map.arena_inward_normal(boundary);
        let start = map
            .arena_boundary()
            .project_inside(boundary, cfg.collision_radius + 2.0)
            .unwrap();
        let mut motion = CompetitorMotion::new(start, -inward);
        let step = 4.0;

        advance_motion(&mut motion, None, &map, &cfg, step, 1.0);

        assert!(motion.position.distance(start) <= step + 0.01);
        assert!(map.arena_signed_distance(motion.position) >= cfg.collision_radius - 0.01);
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
    fn prediction_matches_authoritative_step_for_normal_turn_and_edge() {
        let cfg = GameConfig::default();
        let board = BoardGrid::generate(31, 2, &cfg);
        let map = TerritoryMap::from_board(&board);
        let cases = [
            (Vec2::ZERO, Vec2::X, Some(Vec2::Y)),
            (Vec2::ZERO, Vec2::X, Some(-Vec2::X)),
            (board.contour.points[0], Vec2::X, None),
        ];
        for (position, heading, desired) in cases {
            let motion = CompetitorMotion::new(position, heading);
            let predicted = predict_motion(
                motion,
                desired,
                &map,
                &cfg,
                cfg.player_speed,
                cfg.fixed_delta_seconds(),
            );
            let mut authoritative = motion;
            advance_motion(
                &mut authoritative,
                desired,
                &map,
                &cfg,
                cfg.player_speed,
                cfg.fixed_delta_seconds(),
            );
            assert_eq!(predicted, authoritative);
        }
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
