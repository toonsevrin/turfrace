//! Shared, non-spatial game audio with rate limiting and procedural fallback tones.

use std::{collections::HashMap, time::Duration};

use bevy::{audio::Volume, prelude::*, window::WindowFocused};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioCue {
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
            .add_systems(Update, (track_window_focus, play_cues).chain());
    }
}

#[derive(Resource)]
struct ToneBank(HashMap<AudioCue, Vec<Handle<Pitch>>>);

#[derive(Resource, Default)]
struct RateLimiter(HashMap<AudioCue, f32>);

fn setup_tones(mut commands: Commands, mut pitches: ResMut<Assets<Pitch>>) {
    let specs = [
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
                let ratios: &[f32] = match cue {
                    AudioCue::MenuConfirm | AudioCue::Ready | AudioCue::Respawn => &[1.0, 1.5],
                    AudioCue::Go | AudioCue::Capture | AudioCue::LeaderChange => &[1.0, 1.25],
                    AudioCue::Victory => &[1.0, 1.25, 1.5],
                    AudioCue::TrailCut | AudioCue::SelfCollision | AudioCue::Death => &[1.0, 0.75],
                    _ => &[1.0],
                };
                let layers = ratios
                    .iter()
                    .map(|ratio| pitches.add(Pitch::new(hz * ratio, Duration::from_millis(ms))))
                    .collect();
                (cue, layers)
            })
            .collect(),
    ));
    commands.init_resource::<RateLimiter>();
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
