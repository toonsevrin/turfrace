use bevy::prelude::*;

use crate::{
    geometry::MultiPolygon,
    ids::{CompetitorId, MAX_COMPETITORS},
};

/// Restores the canonical one-island-per-owner invariant after a territory
/// commit and reports every removed component for cache and combat handling.
pub(super) fn retain_spawn_components(
    territories: &mut [MultiPolygon; MAX_COMPETITORS],
    spawn_anchors: &mut [Option<Vec2>; MAX_COMPETITORS],
) -> Vec<(CompetitorId, MultiPolygon)> {
    let mut disconnected = Vec::new();
    for (index, territory) in territories.iter_mut().enumerate() {
        let Some(anchor) = spawn_anchors[index] else {
            continue;
        };
        let anchored = territory
            .polygons
            .iter()
            .find(|polygon| polygon.contains_world(anchor))
            .cloned();
        let Some(anchored) = anchored else {
            if !territory.is_empty() {
                disconnected.push((CompetitorId(index as u8), territory.clone()));
                *territory = MultiPolygon::empty();
            }
            spawn_anchors[index] = None;
            continue;
        };
        if territory.polygons.len() == 1 {
            continue;
        }
        let retained = MultiPolygon {
            polygons: vec![anchored],
        };
        let removed = territory.difference(&retained);
        if !removed.is_empty() {
            disconnected.push((CompetitorId(index as u8), removed));
        }
        *territory = retained;
    }
    disconnected
}
