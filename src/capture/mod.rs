//! Capture geometry facade.
//!
//! The match systems keep the mutable map resource in one place, while this
//! module provides a small pure API for tools and replay tests. There is no
//! raster flood fill or cell-path search here: capture decisions are vector
//! booleans over fixed-point multipolygons.

use crate::{
    ids::CompetitorId,
    territory_map::{TerritoryMap, VectorCaptureResult},
    trail::ActiveTrail,
};

pub type CaptureResult = VectorCaptureResult;

pub fn calculate_capture(
    map: &TerritoryMap,
    player: CompetitorId,
    trail: &ActiveTrail,
    trail_width: f32,
) -> CaptureResult {
    map.calculate_capture(player, trail, trail_width)
}

pub fn apply_capture(map: &mut TerritoryMap, player: CompetitorId, result: &mut CaptureResult) {
    let applied = map.apply_claim(player, result.claim.clone());
    result.claimed_area = applied.claimed_area;
    result.stolen_by_owner = applied.stolen_by_owner;
}

pub fn apply_equal_time_captures(
    map: &mut TerritoryMap,
    captures: &mut [(CompetitorId, CaptureResult)],
) {
    map.apply_equal_time_captures(captures);
}

/// Returns a deterministic vector corridor for visual/debug tooling without
/// exposing the map's internal overlay implementation.
pub fn capture_area(result: &CaptureResult) -> f32 {
    result.claimed_area.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{geometry::MultiPolygon, territory_map::TerritoryMap};
    use bevy::prelude::Vec2;

    fn arena() -> MultiPolygon {
        MultiPolygon::from_outer(&[
            Vec2::new(-20.0, -20.0),
            Vec2::new(20.0, -20.0),
            Vec2::new(20.0, 20.0),
            Vec2::new(-20.0, 20.0),
        ])
    }

    #[test]
    fn bridge_capture_is_vector_corridor_only() {
        let mut map = TerritoryMap::new(arena());
        let player = CompetitorId(0);
        map.claim_disk(Vec2::new(-6.0, 0.0), 3.0, player);
        map.claim_disk(Vec2::new(6.0, 0.0), 3.0, player);
        let mut trail = ActiveTrail::new(
            player,
            crate::board::Cell::new(0, 0),
            Vec2::new(-3.0, 0.0),
            Vec2::X,
        );
        trail.append_exact(Vec2::new(3.0, 0.0));
        let result = calculate_capture(&map, player, &trail, 0.6);
        assert!(!result.used_loop_fill);
        assert!(result.claimed_area > 0.0);
        assert!(result.claim.contains_world(Vec2::ZERO));
    }

    #[test]
    fn equal_time_captures_have_stable_id_priority() {
        let mut map = TerritoryMap::new(arena());
        let high = CompetitorId(1);
        let low = CompetitorId(0);
        let claim = MultiPolygon::from_outer(&[
            Vec2::new(-2.0, -2.0),
            Vec2::new(2.0, -2.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(-2.0, 2.0),
        ]);
        let first = CaptureResult {
            claim: claim.clone(),
            ..Default::default()
        };
        let second = CaptureResult {
            claim,
            ..Default::default()
        };
        let mut captures = vec![(high, first), (low, second)];
        apply_equal_time_captures(&mut map, &mut captures);
        assert!(map.territories[low.index()].contains_world(Vec2::ZERO));
        assert!(!map.territories[high.index()].contains_world(Vec2::ZERO));
    }
}
