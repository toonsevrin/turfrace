use bevy::prelude::*;

use crate::{
    board::BoardGrid,
    config::GameConfig,
    ids::CompetitorId,
    npc::{NpcEvent, NpcEventMessage, NpcEventQueue},
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
    clock: Option<Res<super::model::SimulationClock>>,
    mut npc_events: Option<ResMut<NpcEventQueue>>,
    mut captures: Local<Vec<(CompetitorId, VectorCaptureResult)>>,
    mut query: Query<(&Competitor, &mut MatchStatistics)>,
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
            clock.as_ref().map_or(0, |clock| clock.0),
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
    tick: u64,
    mut npc_events: Option<&mut ResMut<NpcEventQueue>>,
    captures: &mut Vec<(CompetitorId, VectorCaptureResult)>,
    query: &mut Query<(&Competitor, &mut MatchStatistics)>,
) {
    captures.clear();
    captures.extend(pending.iter().map(|capture| {
        (
            capture.player,
            territory.prepare_capture(capture.player, &capture.trail, config.trail_width),
        )
    }));
    // `apply_claim` updates exact geometry and the bounded sample cache as one
    // atomic mutation. There is deliberately no presentation refresh pass here.
    territory.apply_equal_time_captures(captures);

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
        displaced.0.sort_unstable();
        displaced.0.dedup();
        if let Some((_, mut stats)) = query
            .iter_mut()
            .find(|(competitor, _)| competitor.id == pending.player)
        {
            stats.captures_completed += 1;
            stats.area_captured_total += result.claimed_area;
            stats.area_stolen_total += stolen_area;
            stats.largest_capture_area = stats.largest_capture_area.max(result.claimed_area);
        }
        events.0.push(SimulationEvent::Capture {
            player: pending.player,
            area: result.claimed_area,
            stolen_area,
            loop_fill: result.used_loop_fill,
        });
        if let Some(npc_events) = npc_events.as_deref_mut() {
            npc_events.0.push(NpcEventMessage {
                recipient: pending.player,
                event: NpcEvent::OwnCapture {
                    area: result.claimed_area,
                },
                tick,
            });
            npc_events
                .0
                .extend(
                    result
                        .stolen_by_owner
                        .iter()
                        .map(|(victim, area)| NpcEventMessage {
                            recipient: *victim,
                            event: NpcEvent::TerritoryStolen {
                                by: pending.player,
                                area: *area,
                                location: pending.trail.head,
                            },
                            tick,
                        }),
                );
        }
    }
}
