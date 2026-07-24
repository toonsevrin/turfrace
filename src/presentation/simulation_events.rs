use bevy::prelude::*;

use crate::{
    audio::{AudioCue, PlayAudioCue},
    effects::VisualEffect,
    ids::CompetitorId,
    match_game::{
        Competitor, CompetitorKind, DeathCause, KillProgress, MatchPurpose, MatchSession, Rankings,
        SimulationEvent, SimulationEvents,
    },
    movement::CompetitorMotion,
    territory_map::TerritoryMap,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn bridge_simulation_events(
    territory: Option<Res<TerritoryMap>>,
    rankings: Option<Res<Rankings>>,
    session: Option<Res<MatchSession>>,
    events: Option<ResMut<SimulationEvents>>,
    competitors: Query<(Entity, &Competitor, &CompetitorMotion)>,
    mut visual_writer: MessageWriter<VisualEffect>,
    mut audio_writer: MessageWriter<PlayAudioCue>,
    mut last_leader: Local<Option<CompetitorId>>,
) {
    let audio_enabled = should_emit_audio(session.as_deref());
    let mut play = |cue| {
        if audio_enabled {
            audio_writer.write(cue);
        }
    };
    let current_leader = rankings.as_ref().and_then(|rankings| rankings.leader());
    if current_leader.is_none() {
        *last_leader = None;
    }
    let Some(mut events) = events else { return };
    for event in events.drain() {
        match event {
            SimulationEvent::Countdown(_) => {
                play(PlayAudioCue::human(AudioCue::Countdown));
            }
            SimulationEvent::Go => {
                play(PlayAudioCue::human(AudioCue::Go));
            }
            SimulationEvent::TrailStarted { player } => {
                let human = lookup(&competitors, player)
                    .is_some_and(|(_, competitor, _)| competitor.kind == CompetitorKind::Human);
                play(PlayAudioCue {
                    cue: AudioCue::TrailStart,
                    human_involved: human,
                    intensity: 0.3,
                });
            }
            SimulationEvent::Capture { player, area, .. } => {
                if let Some((entity, competitor, motion)) = lookup(&competitors, player) {
                    let percent = territory.as_ref().map_or(0.0, |map| {
                        if map.arena_area > 0.0 {
                            area * 100.0 / map.arena_area
                        } else {
                            0.0
                        }
                    });
                    visual_writer.write(VisualEffect::Capture {
                        source: entity,
                        position: motion.position,
                        color_id: competitor.color_id,
                        percent,
                    });
                    play(PlayAudioCue {
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
                    play(PlayAudioCue {
                        cue,
                        human_involved: competitor.kind == CompetitorKind::Human,
                        intensity: 0.8,
                    });
                }
            }
            SimulationEvent::Kill { killer, progress } => {
                if let Some((entity, competitor, motion)) = lookup(&competitors, killer) {
                    visual_writer.write(VisualEffect::Kill {
                        source: entity,
                        position: motion.position,
                        color_id: competitor.color_id,
                        progress,
                    });
                    play(PlayAudioCue {
                        cue: AudioCue::Kill,
                        human_involved: competitor.kind == CompetitorKind::Human,
                        intensity: kill_audio_intensity(progress),
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
                    play(PlayAudioCue {
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
                    play(PlayAudioCue::human(AudioCue::LeaderChange));
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
                play(PlayAudioCue::human(AudioCue::Victory));
            }
        }
    }
}

fn should_emit_audio(session: Option<&MatchSession>) -> bool {
    session.is_none_or(|session| session.purpose != MatchPurpose::Attract)
}

fn lookup<'a>(
    competitors: &'a Query<(Entity, &Competitor, &CompetitorMotion)>,
    id: CompetitorId,
) -> Option<(Entity, &'a Competitor, &'a CompetitorMotion)> {
    competitors
        .iter()
        .find(|(_, competitor, _)| competitor.id == id)
}

fn kill_audio_intensity(progress: KillProgress) -> f32 {
    (progress.streak.saturating_add(progress.total) as f32 / 10.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use crate::match_game::{KillProgress, MatchPurpose, MatchSession};

    use super::{kill_audio_intensity, should_emit_audio};

    #[test]
    fn saturated_kill_counts_keep_audio_intensity_finite() {
        assert_eq!(
            kill_audio_intensity(KillProgress {
                total: u32::MAX,
                streak: u32::MAX,
            }),
            1.0
        );
        assert_eq!(kill_audio_intensity(KillProgress::default()), 0.0);
    }

    #[test]
    fn attract_events_keep_simulation_audio_quiet() {
        let session = MatchSession {
            purpose: MatchPurpose::Attract,
            ..Default::default()
        };
        assert!(!should_emit_audio(Some(&session)));
        assert!(should_emit_audio(None));
        assert!(should_emit_audio(Some(&MatchSession::default())));
    }
}
