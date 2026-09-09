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
    mut listener_positions: Local<Vec<Vec2>>,
) {
    let Some(mut events) = events else { return };
    let current_leader = rankings.as_ref().and_then(|rankings| rankings.leader());
    if current_leader.is_none() {
        *last_leader = None;
    }
    if events.0.is_empty() {
        return;
    }

    let audio_enabled = should_emit_audio(session.as_deref());
    let mut play = |cue| {
        if audio_enabled {
            audio_writer.write(cue);
        }
    };
    listener_positions.clear();
    listener_positions.extend(
        competitors
            .iter()
            .filter(|(_, competitor, _)| competitor.kind == CompetitorKind::Human)
            .map(|(_, _, motion)| motion.position),
    );
    for event in events.drain() {
        match event {
            SimulationEvent::Countdown(_) => {
                play(PlayAudioCue::human(AudioCue::Countdown));
            }
            SimulationEvent::Go => {
                play(PlayAudioCue::human(AudioCue::Go));
            }
            SimulationEvent::TrailStarted { player } => {
                if let Some((_entity, competitor, motion)) = lookup(&competitors, player) {
                    play(positional_cue(
                        AudioCue::TrailStart,
                        competitor,
                        motion.position,
                        0.3,
                        &listener_positions,
                    ));
                }
            }
            SimulationEvent::Capture { player, area, .. } => {
                if let Some((entity, competitor, motion)) = lookup(&competitors, player) {
                    let percent = territory.as_ref().map_or(0.0, |map| {
                        if map.arena_area() > 0.0 {
                            area * 100.0 / map.arena_area()
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
                    play(positional_cue(
                        AudioCue::Capture,
                        competitor,
                        motion.position,
                        (percent / 10.0).clamp(0.0, 1.0),
                        &listener_positions,
                    ));
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
                    play(positional_cue(
                        cue,
                        competitor,
                        motion.position,
                        0.8,
                        &listener_positions,
                    ));
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
                    play(positional_cue(
                        AudioCue::Kill,
                        competitor,
                        motion.position,
                        kill_audio_intensity(progress),
                        &listener_positions,
                    ));
                }
            }
            SimulationEvent::Respawn { player } => {
                if let Some((entity, competitor, motion)) = lookup(&competitors, player) {
                    visual_writer.write(VisualEffect::Respawn {
                        source: entity,
                        position: motion.position,
                        color_id: competitor.color_id,
                    });
                    play(positional_cue(
                        AudioCue::Respawn,
                        competitor,
                        motion.position,
                        0.5,
                        &listener_positions,
                    ));
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
                        play(positional_cue(
                            AudioCue::LeaderChange,
                            competitor,
                            motion.position,
                            0.5,
                            &listener_positions,
                        ));
                    }
                    *last_leader = current_leader;
                }
            }
            SimulationEvent::Victory { winner } => {
                if let Some((_entity, competitor, motion)) = lookup(&competitors, winner) {
                    visual_writer.write(VisualEffect::Victory {
                        position: motion.position,
                        color_id: competitor.color_id,
                    });
                    play(positional_cue(
                        AudioCue::Victory,
                        competitor,
                        motion.position,
                        1.0,
                        &listener_positions,
                    ));
                }
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

// Gameplay audio is a local awareness cue, not an arena-wide event feed. Keep
// full volume only at immediate contact and make off-screen action inaudible.
const AUDIO_FULL_VOLUME_RADIUS: f32 = 0.75;
const AUDIO_SILENT_RADIUS: f32 = 6.0;

fn positional_cue(
    cue: AudioCue,
    source: &Competitor,
    position: Vec2,
    intensity: f32,
    listeners: &[Vec2],
) -> PlayAudioCue {
    PlayAudioCue {
        cue,
        human_involved: source.kind == CompetitorKind::Human,
        intensity,
        proximity: nearest_listener_gain(position, listeners),
    }
}

fn nearest_listener_gain(source: Vec2, listeners: &[Vec2]) -> f32 {
    let distance = listeners
        .iter()
        .map(|listener| source.distance(*listener))
        .reduce(f32::min)
        .unwrap_or(f32::INFINITY);
    let normalized = ((distance - AUDIO_FULL_VOLUME_RADIUS)
        / (AUDIO_SILENT_RADIUS - AUDIO_FULL_VOLUME_RADIUS))
        .clamp(0.0, 1.0);
    (1.0 - normalized).powi(4)
}

fn kill_audio_intensity(progress: KillProgress) -> f32 {
    (progress.streak.saturating_add(progress.total) as f32 / 10.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use crate::match_game::{KillProgress, MatchPurpose, MatchSession};

    use bevy::prelude::Vec2;

    use super::{kill_audio_intensity, nearest_listener_gain, should_emit_audio};

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
    fn positional_audio_uses_the_nearest_local_player_once() {
        let listeners = [Vec2::ZERO, Vec2::new(30.0, 0.0)];
        assert_eq!(nearest_listener_gain(Vec2::ZERO, &listeners), 1.0);
        assert_eq!(nearest_listener_gain(Vec2::new(30.0, 0.0), &listeners), 1.0);
        assert!(nearest_listener_gain(Vec2::new(2.0, 0.0), &listeners) < 0.35);
        assert!(nearest_listener_gain(Vec2::new(3.0, 0.0), &listeners) < 0.12);
        assert_eq!(nearest_listener_gain(Vec2::new(6.0, 0.0), &listeners), 0.0);
        assert_eq!(
            nearest_listener_gain(Vec2::new(15.0, 15.0), &listeners),
            0.0
        );
        assert_eq!(nearest_listener_gain(Vec2::ZERO, &[]), 0.0);
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
