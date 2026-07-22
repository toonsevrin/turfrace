use bevy::prelude::*;

use crate::{
    ids::CompetitorId,
    trail::{ActiveTrail, swept_self_trail_impact, swept_trail_impact},
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
    bodies: &[CollisionBody],
    trails: &[&ActiveTrail],
    collision_radius: f32,
    trail_width: f32,
    self_exclusion: f32,
) -> Vec<TrailCollisionIntent> {
    let radius = collision_radius + trail_width * 0.5;
    let mut intents = Vec::new();
    for body in bodies {
        if body.protected {
            continue;
        }
        for trail in trails {
            let owner_body = bodies.iter().find(|b| b.id == trail.owner);
            if owner_body.is_some_and(|b| b.protected) {
                continue;
            }
            let impact = if body.id == trail.owner {
                swept_self_trail_impact(
                    body.previous,
                    body.current,
                    radius,
                    &trail.points,
                    self_exclusion,
                )
            } else {
                swept_trail_impact(body.previous, body.current, radius, &trail.points)
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
    use crate::{board::Cell, trail::ActiveTrail};
    #[test]
    fn ties_go_to_lowest_id() {
        let mut t = ActiveTrail::new(
            CompetitorId(2),
            Cell::new(0, 0),
            Vec2::new(0.0, -2.0),
            Vec2::Y,
        );
        t.points.push(Vec2::new(0.0, 2.0));
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
        let i = collect_collision_intents(&bodies, &[&t], 0.1, 0.1, 1.5);
        assert_eq!(i[0].killer, Some(CompetitorId(0)));
    }
}
