use bevy::prelude::*;

use crate::{
    audio::{AudioCue, PlayAudioCue},
    board::BoardGrid,
    effects::VisualEffect,
    ids::CompetitorId,
    match_game::{
        Competitor, CompetitorKind, DeathCause, Rankings, SimulationEvent, SimulationEvents,
    },
    movement::CompetitorMotion,
};

pub(super) fn bridge_simulation_events(
    board: Option<Res<BoardGrid>>,
    rankings: Option<Res<Rankings>>,
    events: Option<ResMut<SimulationEvents>>,
    competitors: Query<(Entity, &Competitor, &CompetitorMotion)>,
    mut visual_writer: MessageWriter<VisualEffect>,
    mut audio_writer: MessageWriter<PlayAudioCue>,
    mut last_leader: Local<Option<CompetitorId>>,
) {
    let current_leader = rankings.as_ref().and_then(|rankings| rankings.leader());
    if current_leader.is_none() {
        *last_leader = None;
    }
    let Some(mut events) = events else { return };
    for event in events.drain() {
        match event {
            SimulationEvent::Countdown(_) => {
                audio_writer.write(PlayAudioCue::human(AudioCue::Countdown));
            }
            SimulationEvent::Go => {
                audio_writer.write(PlayAudioCue::human(AudioCue::Go));
            }
            SimulationEvent::TrailStarted { player } => {
                let human = lookup(&competitors, player)
                    .is_some_and(|(_, competitor, _)| competitor.kind == CompetitorKind::Human);
                audio_writer.write(PlayAudioCue {
                    cue: AudioCue::TrailStart,
                    human_involved: human,
                    intensity: 0.3,
                });
            }
            SimulationEvent::Capture { player, cells, .. } => {
                if let Some((entity, competitor, motion)) = lookup(&competitors, player) {
                    let percent = board.as_ref().map_or(0.0, |board| {
                        cells as f32 * 100.0 / board.playable_cells.max(1) as f32
                    });
                    visual_writer.write(VisualEffect::Capture {
                        source: entity,
                        position: motion.position,
                        color_id: competitor.color_id,
                        percent,
                    });
                    audio_writer.write(PlayAudioCue {
                        cue: AudioCue::Capture,
                        human_involved: competitor.kind == CompetitorKind::Human,
                        intensity: (percent / 10.0).clamp(0.0, 1.0),
                    });
                }
            }
            SimulationEvent::Death { victim, cause, .. } => {
                if let Some((entity, competitor, motion)) = lookup(&competitors, victim) {
                    visual_writer.write(VisualEffect::Death {
                        source: entity,
                        position: motion.position,
                        color_id: competitor.color_id,
                    });
                    let cue = match cause {
                        DeathCause::SelfTrail => AudioCue::SelfCollision,
                        DeathCause::TrailCut => AudioCue::TrailCut,
                        DeathCause::Displaced => AudioCue::Death,
                    };
                    audio_writer.write(PlayAudioCue {
                        cue,
                        human_involved: competitor.kind == CompetitorKind::Human,
                        intensity: 0.8,
                    });
                }
            }
            SimulationEvent::Respawn { player } => {
                if let Some((entity, competitor, motion)) = lookup(&competitors, player) {
                    visual_writer.write(VisualEffect::Respawn {
                        source: entity,
                        position: motion.position,
                        color_id: competitor.color_id,
                    });
                    audio_writer.write(PlayAudioCue {
                        cue: AudioCue::Respawn,
                        human_involved: competitor.kind == CompetitorKind::Human,
                        intensity: 0.5,
                    });
                }
            }
            SimulationEvent::RankingChanged => {
                if current_leader != *last_leader {
                    if let Some(leader) = current_leader
                        && let Some((_entity, competitor, motion)) = lookup(&competitors, leader)
                    {
                        visual_writer.write(VisualEffect::LeaderChanged {
                            position: motion.position,
                            color_id: competitor.color_id,
                        });
                    }
                    audio_writer.write(PlayAudioCue::human(AudioCue::LeaderChange));
                    *last_leader = current_leader;
                }
            }
            SimulationEvent::Victory { winner } => {
                if let Some((_entity, competitor, motion)) = lookup(&competitors, winner) {
                    visual_writer.write(VisualEffect::Victory {
                        position: motion.position,
                        color_id: competitor.color_id,
                    });
                }
                audio_writer.write(PlayAudioCue::human(AudioCue::Victory));
            }
        }
    }
}

fn lookup<'a>(
    competitors: &'a Query<(Entity, &Competitor, &CompetitorMotion)>,
    id: CompetitorId,
) -> Option<(Entity, &'a Competitor, &'a CompetitorMotion)> {
    competitors
        .iter()
        .find(|(_, competitor, _)| competitor.id == id)
}
