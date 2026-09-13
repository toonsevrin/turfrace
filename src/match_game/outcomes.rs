use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::{
    board::BoardGrid,
    ids::CompetitorId,
    npc::{NpcEvent, NpcEventMessage, NpcEventQueue},
    territory_map::TerritoryMap,
    trail::{ActiveTrail, clear_trail_bits},
};

use super::{model::*, rules::MatchRules};

/// A fully classified elimination. Keeping cause and killer together prevents
/// the collision and territory stages from growing separate effect paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EliminationOutcome {
    pub victim: CompetitorId,
    pub killer: Option<CompetitorId>,
    pub cause: DeathCause,
}

impl EliminationOutcome {
    pub fn trail_collision(victim: CompetitorId, killer: Option<CompetitorId>) -> Self {
        Self {
            victim,
            killer,
            cause: if killer.is_some() {
                DeathCause::TrailCut
            } else {
                DeathCause::SelfTrail
            },
        }
    }

    pub fn displaced(victim: CompetitorId, killer: CompetitorId) -> Self {
        Self {
            victim,
            killer: Some(killer),
            cause: DeathCause::Displaced,
        }
    }
}

pub(super) type EliminationQuery = (
    Entity,
    &'static Competitor,
    &'static mut LifeState,
    &'static mut MatchStatistics,
    Option<&'static ActiveTrail>,
);

#[derive(SystemParam)]
pub(super) struct EliminationResources<'w> {
    pub events: ResMut<'w, SimulationEvents>,
    pub session: Option<Res<'w, MatchSession>>,
    pub clock: Option<Res<'w, SimulationClock>>,
    pub feed: Option<ResMut<'w, EliminationFeed>>,
    pub npc_events: Option<ResMut<'w, NpcEventQueue>>,
}

pub(super) struct EliminationTarget<'a> {
    pub entity: Entity,
    pub competitor: CompetitorId,
    pub life: &'a mut LifeState,
    pub stats: &'a mut MatchStatistics,
    pub trail: Option<&'a ActiveTrail>,
}

/// Resolve a sequence of eliminations through one target query. The target
/// borrow is deliberately scoped before the killer lookup so reciprocal
/// same-tick eliminations can both receive credit.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_eliminations(
    outcomes: impl IntoIterator<Item = EliminationOutcome>,
    commands: &mut Commands,
    board: &mut BoardGrid,
    territory: &mut TerritoryMap,
    rules: &MatchRules,
    effects: &mut EliminationResources,
    query: &mut Query<EliminationQuery>,
) {
    for outcome in outcomes {
        let eliminated = {
            let Some((entity, competitor, mut life, mut stats, trail)) = query
                .iter_mut()
                .find(|(_, competitor, _, _, _)| competitor.id == outcome.victim)
            else {
                continue;
            };
            resolve_elimination(
                outcome,
                EliminationTarget {
                    entity,
                    competitor: competitor.id,
                    life: &mut life,
                    stats: &mut stats,
                    trail,
                },
                commands,
                board,
                territory,
                rules,
                effects,
            )
        };
        if eliminated
            && rules.scoring_enabled
            && let Some(killer) = outcome.killer.filter(|killer| *killer != outcome.victim)
            && let Some((_, _, _, mut stats, _)) = query
                .iter_mut()
                .find(|(_, competitor, _, _, _)| competitor.id == killer)
        {
            credit_kill(killer, outcome.victim, &mut stats, effects);
        }
    }
}

/// Apply every consequence of one elimination exactly once.
#[allow(clippy::too_many_arguments)]
fn resolve_elimination(
    outcome: EliminationOutcome,
    target: EliminationTarget<'_>,
    commands: &mut Commands,
    board: &mut BoardGrid,
    territory: &mut TerritoryMap,
    rules: &MatchRules,
    effects: &mut EliminationResources,
) -> bool {
    if target.competitor != outcome.victim || !target.life.is_alive() {
        return false;
    }

    if let Some(trail) = target.trail {
        clear_trail_bits(board, target.competitor, &trail.cells);
        commands.entity(target.entity).remove::<ActiveTrail>();
    }
    territory.clear_owner(target.competitor);

    target.stats.deaths = target.stats.deaths.saturating_add(1);
    target.stats.reset_kill_streak();
    target.life.status = if rules.respawn_enabled {
        LifeStatus::Respawning
    } else {
        LifeStatus::Eliminated
    };
    target.life.respawn_remaining = if rules.respawn_enabled {
        rules.respawn_delay(target.stats.deaths)
    } else {
        0.0
    };

    effects.events.0.push(SimulationEvent::Death {
        victim: outcome.victim,
        killer: outcome.killer,
        cause: outcome.cause,
    });
    if let Some(npc_events) = effects.npc_events.as_deref_mut() {
        npc_events.0.push(NpcEventMessage {
            recipient: outcome.victim,
            event: NpcEvent::Died {
                killer: outcome.killer,
            },
            tick: effects.clock.as_ref().map_or(0, |clock| clock.0),
        });
    }
    let match_time = effects
        .session
        .as_ref()
        .map_or(0.0, |session| session.elapsed_seconds);
    if let Some(feed) = effects.feed.as_deref_mut() {
        feed.push(EliminationRecord {
            victim: outcome.victim,
            killer: outcome.killer,
            cause: outcome.cause,
            match_time,
        });
    }

    true
}

fn credit_kill(
    killer: CompetitorId,
    victim: CompetitorId,
    stats: &mut MatchStatistics,
    effects: &mut EliminationResources,
) {
    let progress = stats.record_kill();
    effects
        .events
        .0
        .push(SimulationEvent::Kill { killer, progress });
    if let Some(npc_events) = effects.npc_events.as_deref_mut() {
        npc_events.0.push(NpcEventMessage {
            recipient: killer,
            event: NpcEvent::CreditedKill { victim },
            tick: effects.clock.as_ref().map_or(0, |clock| clock.0),
        });
    }
}
