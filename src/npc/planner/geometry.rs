use super::*;
use crate::territory_map::TerritoryMap;
use bevy::prelude::*;

pub(super) fn valid_route_geometry(
    territory: &TerritoryMap,
    margin: f32,
    route: &NpcRoute,
    start: Vec2,
) -> bool {
    let mut previous = start;
    for point in route.points.iter().take(route.count as usize) {
        if !point.is_finite() {
            return false;
        }
        for step in 0..=ROUTE_SAMPLE_CAP {
            let sample = previous.lerp(*point, step as f32 / ROUTE_SAMPLE_CAP as f32);
            if !territory.arena_boundary().at_least_margin(sample, margin) {
                return false;
            }
        }
        previous = *point;
    }
    true
}

pub(super) fn return_ownership_valid(
    territory: &TerritoryMap,
    id: CompetitorId,
    route: &NpcRoute,
    start: Vec2,
) -> bool {
    let mut previous = start;
    let mut reentered = false;
    let initial_owner = territory.owner_at(start);
    let mut exited_initial_enemy = false;
    for point in route.points.iter().take(route.count as usize) {
        for step in 0..=ROUTE_SAMPLE_CAP {
            let sample = previous.lerp(*point, step as f32 / ROUTE_SAMPLE_CAP as f32);
            let owner = territory.owner_at(sample);
            if owner != crate::ids::OwnerId::UNCLAIMED && owner != id.owner() {
                // A raider already inside enemy land must be allowed to exit
                // it. Do not permit entering another enemy region on return.
                if owner != initial_owner || exited_initial_enemy {
                    return false;
                }
            } else {
                exited_initial_enemy = true;
            }
            if territory.owns(sample, id) {
                reentered = true;
            } else if reentered {
                // Once a return has reached owned ground it must not leave it
                // again on the way to the exact final anchor.
                return false;
            }
        }
        previous = *point;
    }
    reentered
}

pub(super) fn raid_ownership_valid(
    territory: &TerritoryMap,
    id: CompetitorId,
    route: &NpcRoute,
    start: Vec2,
) -> bool {
    let RouteTarget::EnemyBorder(target_owner) = route.target else {
        return false;
    };
    let mut previous = start;
    let mut reentered = false;
    for point in route.points.iter().take(route.count as usize) {
        for step in 0..=ROUTE_SAMPLE_CAP {
            let sample = previous.lerp(*point, step as f32 / ROUTE_SAMPLE_CAP as f32);
            let owner = territory.owner_at(sample);
            if owner != crate::ids::OwnerId::UNCLAIMED
                && owner != id.owner()
                && owner != target_owner.owner()
            {
                return false;
            }
            if territory.owns(sample, id) {
                reentered = true;
            } else if reentered {
                return false;
            }
        }
        previous = *point;
    }
    reentered
}

pub(super) fn route_crosses_owner(
    territory: &TerritoryMap,
    target: RouteTarget,
    route: &NpcRoute,
    cell_size: f32,
) -> bool {
    let RouteTarget::EnemyBorder(owner) = target else {
        return true;
    };
    if route.count == 0 {
        return false;
    }
    if territory.owner_at(route.points[0]) == owner.owner() {
        return true;
    }
    let mut previous = route.points[0];
    for point in route.points.iter().take(route.count as usize).skip(1) {
        let distance = previous.distance(*point);
        let steps = (distance / cell_size.max(0.5)).ceil().clamp(1.0, 24.0) as usize;
        if (0..=steps).any(|step| {
            territory.owner_at(previous.lerp(*point, step as f32 / steps as f32)) == owner.owner()
        }) {
            return true;
        }
        previous = *point;
    }
    false
}

/// Bounded swept samples protect against returning over an enemy pocket or own trail.
pub fn segment_has_ownership(
    territory: &TerritoryMap,
    id: CompetitorId,
    start: Vec2,
    end: Vec2,
    owned: bool,
) -> bool {
    (0..=ROUTE_SAMPLE_CAP).all(|step| {
        territory.owns(start.lerp(end, step as f32 / ROUTE_SAMPLE_CAP as f32), id) == owned
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{board::BoardGrid, config::GameConfig};

    #[test]
    fn return_can_exit_enemy_land_but_cannot_enter_it_from_safety() {
        let board = BoardGrid::generate(91, 2, &GameConfig::default());
        let mut territory = TerritoryMap::from_board(&board);
        let home = Vec2::new(-12.0, 0.0);
        let enemy = Vec2::new(5.0, 0.0);
        territory.seed_owner(home, 3.5, CompetitorId(0));
        territory.seed_owner(enemy, 3.5, CompetitorId(1));
        let route = NpcRoute::from_points(&[home], RouteTarget::OwnedGround);
        assert!(return_ownership_valid(
            &territory,
            CompetitorId(0),
            &route,
            enemy
        ));
        let detour = NpcRoute::from_points(&[enemy, home], RouteTarget::OwnedGround);
        assert!(!return_ownership_valid(
            &territory,
            CompetitorId(0),
            &detour,
            Vec2::ZERO
        ));
        let reenter_enemy =
            NpcRoute::from_points(&[Vec2::ZERO, enemy, home], RouteTarget::OwnedGround);
        assert!(!return_ownership_valid(
            &territory,
            CompetitorId(0),
            &reenter_enemy,
            enemy
        ));
    }
}
