use bevy::prelude::*;

use crate::{
    board::BoardGrid,
    config::GameConfig,
    ids::CompetitorId,
    npc::{NpcEvent, NpcEventQueue},
    territory_map::{TerritoryMap, VectorCaptureResult},
    trail::{ActiveTrail, clear_trail_bits},
};

use super::model::{
    Competitor, DisplacementCredits, LifeState, MatchStatistics, PendingCapture, PendingCaptures,
    SimulationEvent, SimulationEvents,
};

pub(super) fn detect_closures(
    territory: Res<TerritoryMap>,
    mut pending: ResMut<PendingCaptures>,
    query: Query<(
        Entity,
        &Competitor,
        &crate::movement::CompetitorMotion,
        &LifeState,
        &ActiveTrail,
    )>,
) {
    pending.0.clear();
    for (entity, competitor, motion, life, trail) in &query {
        if life.is_alive() && territory.owns(motion.position, competitor.id) {
            pending.0.push(PendingCapture {
                player: competitor.id,
                entity,
                time: territory
                    .boundary_entry_time(competitor.id, motion.previous_position, motion.position)
                    .unwrap_or(1.0),
                trail: trail.clone(),
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_captures(
    mut commands: Commands,
    config: Res<GameConfig>,
    mut board: ResMut<BoardGrid>,
    mut territory: ResMut<TerritoryMap>,
    mut pending: ResMut<PendingCaptures>,
    mut displaced: ResMut<DisplacementCredits>,
    mut events: ResMut<SimulationEvents>,
    mut npc_events: Option<ResMut<NpcEventQueue>>,
    mut captures: Local<Vec<(CompetitorId, VectorCaptureResult)>>,
    mut query: Query<(
        &Competitor,
        &crate::movement::CompetitorMotion,
        &mut MatchStatistics,
    )>,
) {
    pending.0.sort_by(|a, b| {
        a.time
            .total_cmp(&b.time)
            .then_with(|| a.player.cmp(&b.player))
    });
    let items = std::mem::take(&mut pending.0);
    let mut offset = 0;
    while offset < items.len() {
        let mut end = offset + 1;
        while end < items.len() && (items[end].time - items[offset].time).abs() <= 1e-5 {
            end += 1;
        }
        resolve_capture_group(
            &items[offset..end],
            &mut commands,
            &config,
            &mut board,
            &mut territory,
            &mut displaced,
            &mut events,
            npc_events.as_mut(),
            &mut captures,
            &mut query,
        );
        offset = end;
    }
    pending.0 = items;
    pending.0.clear();
}

#[allow(clippy::too_many_arguments)]
fn resolve_capture_group(
    pending: &[PendingCapture],
    commands: &mut Commands,
    config: &GameConfig,
    board: &mut BoardGrid,
    territory: &mut TerritoryMap,
    displaced: &mut DisplacementCredits,
    events: &mut SimulationEvents,
    mut npc_events: Option<&mut ResMut<NpcEventQueue>>,
    captures: &mut Vec<(CompetitorId, VectorCaptureResult)>,
    query: &mut Query<(
        &Competitor,
        &crate::movement::CompetitorMotion,
        &mut MatchStatistics,
    )>,
) {
    captures.clear();
    captures.extend(pending.iter().map(|capture| {
        (
            capture.player,
            territory.prepare_capture(capture.player, &capture.trail, config.trail_width),
        )
    }));
    let revision = territory.revision;
    territory.apply_equal_time_captures(captures);
    if territory.revision != revision {
        // Refresh each changed AABB directly. Unioning all capture geometry
        // merely to derive one bounding box can cost more than the bounded
        // sample scans themselves for a large late-match lobe.
        for (_, result) in captures.iter() {
            territory.refresh_sample_cache(board, &result.claim);
            for (_, removed) in &result.disconnected_by_owner {
                territory.refresh_sample_cache(board, removed);
            }
        }
    }

    for pending in pending {
        clear_trail_bits(board, pending.player, &pending.trail.cells);
        commands.entity(pending.entity).remove::<ActiveTrail>();
        let result = &captures
            .iter()
            .find(|(player, _)| *player == pending.player)
            .expect("each pending capture has one calculated result")
            .1;
        let stolen_area = result.stolen_by_owner.iter().map(|(_, area)| *area).sum();
        displaced.0.extend(
            result
                .stolen_by_owner
                .iter()
                .filter(|(victim, _)| territory.area(*victim) <= 1e-4)
                .map(|(victim, _)| (*victim, pending.player)),
        );
        displaced.0.extend(
            query
                .iter()
                .filter(|(competitor, motion, _)| {
                    is_on_disconnected_section(
                        &result.disconnected_by_owner,
                        competitor.id,
                        motion.position,
                    )
                })
                .map(|(competitor, _, _)| (competitor.id, pending.player)),
        );
        displaced.0.sort_unstable();
        displaced.0.dedup();
        let cell_area = board.cell_size * board.cell_size;
        let cells = area_as_cells(result.claimed_area, cell_area);
        let stolen = area_as_cells(stolen_area, cell_area);
        if let Some((_, _, mut stats)) = query
            .iter_mut()
            .find(|(competitor, _, _)| competitor.id == pending.player)
        {
            stats.captures_completed += 1;
            stats.area_captured_total += result.claimed_area;
            stats.area_stolen_total += stolen_area;
            stats.largest_capture_area = stats.largest_capture_area.max(result.claimed_area);
            stats.cells_captured_total += cells;
            stats.cells_stolen_total += stolen;
            stats.largest_capture_cells = stats.largest_capture_cells.max(cells);
        }
        events.0.push(SimulationEvent::Capture {
            player: pending.player,
            area: result.claimed_area,
            stolen_area,
            cells,
            stolen,
            loop_fill: result.used_loop_fill,
        });
        if let Some(npc_events) = npc_events.as_deref_mut() {
            npc_events.0.push((
                pending.player,
                NpcEvent::OwnCapture {
                    area: result.claimed_area,
                },
            ));
            npc_events
                .0
                .extend(result.stolen_by_owner.iter().map(|(victim, area)| {
                    (
                        *victim,
                        NpcEvent::TerritoryStolen {
                            by: pending.player,
                            area: *area,
                        },
                    )
                }));
        }
    }
}

fn is_on_disconnected_section(
    disconnected: &[(CompetitorId, crate::geometry::MultiPolygon)],
    player: CompetitorId,
    position: Vec2,
) -> bool {
    disconnected
        .iter()
        .any(|(owner, removed)| *owner == player && removed.contains_world(position))
}

fn area_as_cells(area: f32, cell_area: f32) -> u32 {
    (area / cell_area).round().max(0.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::MultiPolygon;

    #[test]
    fn only_players_standing_on_the_severed_island_are_displaced() {
        let victim = CompetitorId(1);
        let removed = MultiPolygon::from_outer(&[
            Vec2::new(4.0, -2.0),
            Vec2::new(8.0, -2.0),
            Vec2::new(8.0, 2.0),
            Vec2::new(4.0, 2.0),
        ]);
        let disconnected = vec![(victim, removed)];
        assert!(is_on_disconnected_section(
            &disconnected,
            victim,
            Vec2::new(6.0, 0.0)
        ));
        assert!(!is_on_disconnected_section(
            &disconnected,
            victim,
            Vec2::ZERO
        ));
        assert!(!is_on_disconnected_section(
            &disconnected,
            CompetitorId(2),
            Vec2::new(6.0, 0.0)
        ));
    }
}
