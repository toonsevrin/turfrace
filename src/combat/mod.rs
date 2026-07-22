use bevy::prelude::*;

use crate::{
    board::BoardGrid,
    ids::CompetitorId,
    ids::MAX_COMPETITORS,
    trail::{ActiveTrail, swept_active_trail_impact, swept_self_active_trail_impact},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionBody {
    pub id: CompetitorId,
    pub previous: Vec2,
    pub current: Vec2,
    pub protected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailCollisionIntent {
    pub victim: CompetitorId,
    pub killer: Option<CompetitorId>,
    pub impact_time: f32,
}

/// Collects all intents from a snapshot; callers apply deaths only after this returns.
pub fn collect_collision_intents(
    board: &BoardGrid,
    bodies: &[CollisionBody],
    trails: &[&ActiveTrail],
    collision_radius: f32,
    trail_width: f32,
    self_exclusion: f32,
) -> Vec<TrailCollisionIntent> {
    let radius = collision_radius + trail_width * 0.5;
    let mut trail_by_owner: [Option<&ActiveTrail>; MAX_COMPETITORS] = [None; MAX_COMPETITORS];
    let mut protection = [false; MAX_COMPETITORS];
    for trail in trails {
        trail_by_owner[trail.owner.index()] = Some(*trail);
    }
    for body in bodies {
        protection[body.id.index()] = body.protected;
    }
    let mut intents = Vec::new();
    for body in bodies {
        if body.protected {
            continue;
        }
        let mut candidates: [Vec<usize>; MAX_COMPETITORS] = std::array::from_fn(|_| Vec::new());
        let Some((min, max)) = board.clamped_cell_bounds(
            body.previous.min(body.current) - Vec2::splat(radius),
            body.previous.max(body.current) + Vec2::splat(radius),
        ) else {
            continue;
        };
        for y in min.y..=max.y {
            for x in min.x..=max.x {
                let index = board.index(crate::board::Cell::new(x, y)).unwrap();
                for reference in &board.trail_segment_buckets[index] {
                    candidates[reference.owner.index()].push(reference.segment);
                }
            }
        }
        for (owner, segments) in candidates.iter_mut().enumerate() {
            if segments.is_empty() || protection[owner] {
                continue;
            }
            segments.sort_unstable();
            segments.dedup();
            let Some(trail) = trail_by_owner[owner] else {
                continue;
            };
            let impact = if body.id == trail.owner {
                swept_self_active_trail_impact(
                    body.previous,
                    body.current,
                    radius,
                    trail,
                    self_exclusion,
                    segments,
                )
            } else {
                swept_active_trail_impact(body.previous, body.current, radius, trail, segments)
            };
            if let Some(impact_time) = impact {
                intents.push(TrailCollisionIntent {
                    victim: trail.owner,
                    killer: (body.id != trail.owner).then_some(body.id),
                    impact_time,
                });
            }
        }
    }
    intents.sort_by(|a, b| {
        a.victim
            .cmp(&b.victim)
            .then_with(|| a.impact_time.total_cmp(&b.impact_time))
            .then_with(|| a.killer.cmp(&b.killer))
    });
    intents.dedup_by_key(|intent| intent.victim);
    intents
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{board::Cell, config::GameConfig, trail::ActiveTrail, trail::update_trail_raster};
    #[test]
    fn ties_go_to_lowest_id() {
        let mut t = ActiveTrail::new(
            CompetitorId(2),
            Cell::new(0, 0),
            Vec2::new(0.0, -2.0),
            Vec2::Y,
        );
        t.append_exact(Vec2::new(0.0, 2.0));
        let bodies = [
            CollisionBody {
                id: CompetitorId(1),
                previous: Vec2::new(-1.0, 0.0),
                current: Vec2::new(1.0, 0.0),
                protected: false,
            },
            CollisionBody {
                id: CompetitorId(0),
                previous: Vec2::new(-1.0, 0.0),
                current: Vec2::new(1.0, 0.0),
                protected: false,
            },
            CollisionBody {
                id: CompetitorId(2),
                previous: Vec2::new(2.0, 2.0),
                current: Vec2::new(2.0, 2.0),
                protected: false,
            },
        ];
        let mut board = crate::board::BoardGrid::generate(5, 2, &GameConfig::default());
        update_trail_raster(&mut board, &mut t, 0.1);
        let i = collect_collision_intents(&board, &bodies, &[&t], 0.1, 0.1, 1.5);
        assert_eq!(i[0].killer, Some(CompetitorId(0)));
    }

    #[test]
    fn distant_history_is_rejected_by_cell_broadphase() {
        let config = GameConfig::default();
        let mut board = crate::board::BoardGrid::generate(9, 2, &config);
        let mut trail = ActiveTrail::new(
            CompetitorId(1),
            Cell::new(10, 10),
            Vec2::new(-20.0, -20.0),
            Vec2::X,
        );
        trail.append_exact(Vec2::new(20.0, -20.0));
        update_trail_raster(&mut board, &mut trail, config.trail_width);
        let bodies = [CollisionBody {
            id: CompetitorId(0),
            previous: Vec2::new(0.0, 20.0),
            current: Vec2::new(0.0, 20.0),
            protected: false,
        }];
        assert!(
            collect_collision_intents(
                &board,
                &bodies,
                &[&trail],
                config.collision_radius,
                config.trail_width,
                config.self_trail_exclusion_distance,
            )
            .is_empty()
        );
    }
}
