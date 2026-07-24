//! Shared, non-spatial game audio with rate limiting and procedural fallback tones.

use std::{collections::HashMap, time::Duration};

use bevy::{audio::Volume, prelude::*, window::WindowFocused};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioCue {
    Ambient,
    MenuMove,
    MenuConfirm,
    MenuBack,
    PlayerJoin,
    Ready,
    Countdown,
    Go,
    TrailStart,
    Capture,
    Kill,
    TrailCut,
    SelfCollision,
    Death,
    Respawn,
    LeaderChange,
    Victory,
    Pause,
    Resume,
}

#[derive(Message, Debug, Clone, Copy)]
pub struct PlayAudioCue {
    pub cue: AudioCue,
    pub human_involved: bool,
    /// Capture size and similar emphasis, normalized to 0..1.
    pub intensity: f32,
}

impl PlayAudioCue {
    pub fn human(cue: AudioCue) -> Self {
        Self {
            cue,
            human_involved: true,
            intensity: 0.5,
        }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct AudioSettings {
    pub master_volume: f32,
    pub music_volume: f32,
    pub effects_volume: f32,
    pub muted: bool,
    pub mute_when_unfocused: bool,
    focused: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master_volume: 0.72,
            music_volume: 0.55,
            effects_volume: 0.78,
            muted: false,
            mute_when_unfocused: true,
            focused: true,
        }
    }
}

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioSettings>()
            .add_message::<PlayAudioCue>()
            .add_systems(Startup, setup_tones)
            .init_resource::<AmbientClock>()
            .add_systems(Update, (track_window_focus, play_cues).chain())
            .add_systems(
                Update,
                play_ambient.run_if(in_state(crate::app_state::AppState::Playing)),
            );
    }
}

#[derive(Resource)]
struct ToneBank(HashMap<AudioCue, Vec<Handle<Pitch>>>);

#[derive(Resource, Default)]
struct RateLimiter(HashMap<AudioCue, f32>);

#[derive(Resource)]
struct AmbientClock {
    remaining: f32,
    step: u8,
}

impl Default for AmbientClock {
    fn default() -> Self {
        Self {
            remaining: 0.0,
            step: 0,
        }
    }
}

fn setup_tones(mut commands: Commands, mut pitches: ResMut<Assets<Pitch>>) {
    let specs = [
        (AudioCue::Ambient, 220.0, 700),
        (AudioCue::MenuMove, 420.0, 45),
        (AudioCue::MenuConfirm, 660.0, 75),
        (AudioCue::MenuBack, 280.0, 70),
        (AudioCue::PlayerJoin, 520.0, 100),
        (AudioCue::Ready, 740.0, 90),
        (AudioCue::Countdown, 440.0, 110),
        (AudioCue::Go, 880.0, 190),
        (AudioCue::TrailStart, 330.0, 75),
        (AudioCue::Capture, 620.0, 140),
        (AudioCue::Kill, 760.0, 150),
        (AudioCue::TrailCut, 190.0, 130),
        (AudioCue::SelfCollision, 145.0, 180),
        (AudioCue::Death, 110.0, 260),
        (AudioCue::Respawn, 700.0, 150),
        (AudioCue::LeaderChange, 790.0, 110),
        (AudioCue::Victory, 990.0, 420),
        (AudioCue::Pause, 250.0, 90),
        (AudioCue::Resume, 500.0, 90),
    ];
    commands.insert_resource(ToneBank(
        specs
            .into_iter()
            .map(|(cue, hz, ms)| {
                let layers = tone_ratios(cue)
                    .iter()
                    .map(|ratio| pitches.add(Pitch::new(hz * ratio, Duration::from_millis(ms))))
                    .collect();
                (cue, layers)
            })
            .collect(),
    ));
    commands.init_resource::<RateLimiter>();
}

/// Compact harmonic signatures make events identifiable without streamed
/// assets. Consonant upward stacks reward progress; downward and sub-octave
/// stacks communicate danger even when the screen is busy.
fn tone_ratios(cue: AudioCue) -> &'static [f32] {
    match cue {
        AudioCue::Ambient => &[1.0, 1.0, 1.0, 1.0],
        AudioCue::MenuMove => &[1.0, 2.0],
        AudioCue::MenuConfirm | AudioCue::Ready | AudioCue::Respawn => &[1.0, 1.5, 2.0],
        AudioCue::PlayerJoin => &[1.0, 1.25, 1.5],
        AudioCue::Go | AudioCue::LeaderChange => &[1.0, 1.25, 1.5],
        AudioCue::Capture => &[0.5, 1.0, 1.25, 1.5],
        AudioCue::Kill => &[0.5, 1.0, 1.5, 2.0],
        AudioCue::Victory => &[0.5, 1.0, 1.25, 1.5, 2.0],
        AudioCue::TrailCut | AudioCue::SelfCollision => &[1.0, 0.75, 0.5],
        AudioCue::Death => &[1.0, 0.75, 0.5, 0.375],
        AudioCue::Pause => &[1.0, 0.5],
        AudioCue::Resume => &[0.5, 1.0],
        _ => &[1.0],
    }
}

fn ambient_step_hz(step: u8) -> f32 {
    // A minor-pentatonic four-note bed stays legible below gameplay sounds and
    // never needs a decoded music asset or a browser fetch.
    [220.0, 261.63, 293.66, 329.63][usize::from(step % 4)]
}

fn play_ambient(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<AudioSettings>,
    bank: Res<ToneBank>,
    mut clock: ResMut<AmbientClock>,
) {
    clock.remaining -= time.delta_secs();
    if clock.remaining > 0.0
        || settings.muted
        || (settings.mute_when_unfocused && !settings.focused)
    {
        return;
    }
    let Some(tones) = bank.0.get(&AudioCue::Ambient) else {
        return;
    };
    let step = clock.step;
    clock.step = clock.step.wrapping_add(1);
    clock.remaining = 1.55;
    let index = (step % 4) as usize;
    if let Some(tone) = tones.get(index.min(tones.len().saturating_sub(1))) {
        commands.spawn((
            AudioPlayer(tone.clone()),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(
                    (settings.master_volume * settings.music_volume * 0.045).clamp(0.0, 1.0),
                ))
                .with_speed((ambient_step_hz(step) / 220.0).clamp(0.5, 2.0)),
        ));
    }
}

fn play_cues(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<AudioSettings>,
    bank: Res<ToneBank>,
    mut limiter: ResMut<RateLimiter>,
    mut events: MessageReader<PlayAudioCue>,
) {
    let now = time.elapsed_secs();
    for event in events.read() {
        let min_interval = match event.cue {
            AudioCue::Capture | AudioCue::TrailStart => 0.055,
            _ => 0.018,
        };
        if limiter
            .0
            .get(&event.cue)
            .is_some_and(|last| now - *last < min_interval)
        {
            continue;
        }
        limiter.0.insert(event.cue, now);
        if settings.muted || (settings.mute_when_unfocused && !settings.focused) {
            continue;
        }
        let Some(tones) = bank.0.get(&event.cue) else {
            continue;
        };
        let involvement = if event.human_involved { 1.0 } else { 0.72 };
        let emphasis = 0.88 + event.intensity.clamp(0.0, 1.0) * 0.22;
        let volume =
            (settings.master_volume * settings.effects_volume * involvement * 0.18).clamp(0.0, 1.0);
        let layer_volume = volume / (tones.len() as f32).sqrt();
        for tone in tones {
            commands.spawn((
                AudioPlayer(tone.clone()),
                PlaybackSettings::DESPAWN
                    .with_volume(Volume::Linear(layer_volume))
                    .with_speed(emphasis),
            ));
        }
    }
}

fn track_window_focus(
    mut events: MessageReader<WindowFocused>,
    mut settings: ResMut<AudioSettings>,
) {
    for event in events.read() {
        settings.focused = event.focused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_and_danger_cues_have_opposite_harmonic_motion() {
        assert!(
            tone_ratios(AudioCue::Capture)
                .iter()
                .any(|ratio| *ratio > 1.0)
        );
        assert!(
            tone_ratios(AudioCue::Death)
                .iter()
                .any(|ratio| *ratio < 0.5)
        );
    }

    #[test]
    fn ambient_bed_uses_a_repeatable_four_note_scale() {
        assert_eq!(ambient_step_hz(0), ambient_step_hz(4));
        assert_ne!(ambient_step_hz(0), ambient_step_hz(1));
    }
}
