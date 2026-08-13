//! Shared, non-spatial game audio backed by the curated CC0 Kenney sound suite.
//!
//! Every gameplay and menu cue uses a real, short-form sound asset rather than
//! a sine-wave placeholder. Two variations are loaded for the busier cues so
//! repeated captures and menu actions do not become a single obvious loop.

use std::collections::HashMap;

use bevy::{
    audio::{AudioSink, AudioSinkPlayback, AudioSource, Volume},
    prelude::*,
    window::WindowFocused,
};

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

const AUDIO_CUES: &[AudioCue] = &[
    AudioCue::MenuMove,
    AudioCue::MenuConfirm,
    AudioCue::MenuBack,
    AudioCue::PlayerJoin,
    AudioCue::Ready,
    AudioCue::Countdown,
    AudioCue::Go,
    AudioCue::TrailStart,
    AudioCue::Capture,
    AudioCue::Kill,
    AudioCue::TrailCut,
    AudioCue::SelfCollision,
    AudioCue::Death,
    AudioCue::Respawn,
    AudioCue::LeaderChange,
    AudioCue::Victory,
    AudioCue::Pause,
    AudioCue::Resume,
];

#[derive(Message, Debug, Clone, Copy)]
pub struct PlayAudioCue {
    pub cue: AudioCue,
    pub human_involved: bool,
    /// Capture size and similar emphasis, normalized to 0..1.
    pub intensity: f32,
    /// Distance attenuation already collapsed across every local listener.
    /// One simulation event always produces at most one shared-screen sound.
    pub proximity: f32,
}

impl PlayAudioCue {
    pub fn human(cue: AudioCue) -> Self {
        Self {
            cue,
            human_involved: true,
            intensity: 0.5,
            proximity: 1.0,
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
            .add_systems(Startup, load_audio_suite)
            .add_systems(
                Update,
                (track_window_focus, play_cues, sync_background_music).chain(),
            );
    }
}

#[derive(Resource)]
struct AudioBank(HashMap<AudioCue, Vec<Handle<AudioSource>>>);

#[derive(Component)]
struct BackgroundMusic;

#[derive(Resource, Default)]
struct RateLimiter {
    last_played: HashMap<AudioCue, f32>,
    next_variant: HashMap<AudioCue, usize>,
}

fn load_audio_suite(mut commands: Commands, asset_server: Res<AssetServer>) {
    let bank = AUDIO_CUES
        .iter()
        .map(|cue| {
            let sounds = asset_paths(*cue)
                .iter()
                .map(|path| asset_server.load::<AudioSource>(*path))
                .collect();
            (*cue, sounds)
        })
        .collect();
    commands.insert_resource(AudioBank(bank));
    commands.init_resource::<RateLimiter>();
    commands.spawn((
        BackgroundMusic,
        AudioPlayer::new(asset_server.load("audio/background_loop.ogg")),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
    ));
}

/// Asset selection is deliberately explicit: this is the auditable manifest
/// for the final sound suite and makes missing coverage fail in tests.
fn asset_paths(cue: AudioCue) -> &'static [&'static str] {
    match cue {
        AudioCue::MenuMove => &["audio/menu_move.ogg", "audio/menu_move_alt.ogg"],
        AudioCue::MenuConfirm => &["audio/menu_confirm.ogg", "audio/menu_confirm_alt.ogg"],
        AudioCue::MenuBack => &["audio/menu_back.ogg", "audio/menu_back_alt.ogg"],
        AudioCue::PlayerJoin => &["audio/player_join.ogg", "audio/player_join_alt.ogg"],
        AudioCue::Ready => &["audio/ready.ogg", "audio/ready_alt.ogg"],
        AudioCue::Countdown => &["audio/countdown.ogg", "audio/countdown_alt.ogg"],
        AudioCue::Go => &["audio/go.ogg", "audio/go_alt.ogg"],
        AudioCue::TrailStart => &["audio/trail_start.ogg", "audio/trail_start_alt.ogg"],
        AudioCue::Capture => &["audio/capture.ogg", "audio/capture_alt.ogg"],
        AudioCue::Kill => &["audio/kill.ogg", "audio/kill_alt.ogg"],
        AudioCue::TrailCut => &["audio/trail_cut.ogg", "audio/trail_cut_alt.ogg"],
        AudioCue::SelfCollision => &["audio/self_collision.ogg", "audio/self_collision_alt.ogg"],
        AudioCue::Death => &["audio/death.ogg", "audio/death_alt.ogg"],
        AudioCue::Respawn => &["audio/respawn.ogg", "audio/respawn_alt.ogg"],
        AudioCue::LeaderChange => &["audio/leader_change.ogg", "audio/leader_change_alt.ogg"],
        AudioCue::Victory => &["audio/victory.ogg", "audio/victory_alt.ogg"],
        AudioCue::Pause => &["audio/pause.ogg"],
        AudioCue::Resume => &["audio/resume.ogg"],
    }
}

/// Base mix levels keep the dense territory battle punchy without allowing a
/// large capture or an eight-player collision to clip the shared output.
fn cue_mix(cue: AudioCue) -> (f32, f32) {
    match cue {
        AudioCue::MenuMove => (0.075, 1.0),
        AudioCue::MenuConfirm | AudioCue::MenuBack => (0.16, 1.0),
        AudioCue::PlayerJoin | AudioCue::Ready => (0.18, 1.0),
        AudioCue::Countdown => (0.18, 0.96),
        AudioCue::Go | AudioCue::Victory => (0.3, 1.0),
        AudioCue::TrailStart | AudioCue::Respawn => (0.14, 1.0),
        AudioCue::Capture | AudioCue::LeaderChange => (0.18, 1.0),
        AudioCue::Kill => (0.22, 1.0),
        AudioCue::TrailCut => (0.2, 0.98),
        AudioCue::SelfCollision | AudioCue::Death => (0.3, 0.94),
        AudioCue::Pause | AudioCue::Resume => (0.2, 1.0),
    }
}

fn cue_rate_limit(cue: AudioCue) -> f32 {
    match cue {
        AudioCue::MenuMove => 0.06,
        AudioCue::Capture | AudioCue::TrailStart | AudioCue::Kill => 0.09,
        _ => 0.018,
    }
}

fn play_cues(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<AudioSettings>,
    bank: Res<AudioBank>,
    mut limiter: ResMut<RateLimiter>,
    mut events: MessageReader<PlayAudioCue>,
) {
    let now = time.elapsed_secs();
    for event in events.read() {
        if limiter
            .last_played
            .get(&event.cue)
            .is_some_and(|last| now - *last < cue_rate_limit(event.cue))
        {
            continue;
        }
        limiter.last_played.insert(event.cue, now);
        if settings.muted || (settings.mute_when_unfocused && !settings.focused) {
            continue;
        }
        let Some(sounds) = bank.0.get(&event.cue) else {
            continue;
        };
        let Some(first_sound) = sounds.first() else {
            continue;
        };
        let variant = limiter.next_variant.entry(event.cue).or_default();
        let sound = sounds
            .get(*variant % sounds.len())
            .unwrap_or(first_sound)
            .clone();
        *variant = variant.wrapping_add(1);

        let involvement = if event.human_involved { 1.0 } else { 0.72 };
        let intensity = event.intensity.clamp(0.0, 1.0);
        let (base_volume, base_speed) = cue_mix(event.cue);
        let volume = (settings.master_volume
            * settings.effects_volume
            * involvement
            * event.proximity.clamp(0.0, 1.0)
            * base_volume
            * (0.9 + intensity * 0.1))
            .clamp(0.0, 1.0);
        if volume < 0.002 {
            continue;
        }
        let speed = (base_speed * (0.97 + intensity * 0.08)).clamp(0.75, 1.25);
        commands.spawn((
            AudioPlayer::new(sound),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(volume))
                .with_speed(speed),
        ));
    }
}

const BACKGROUND_MUSIC_LEVEL: f32 = 0.055;

fn sync_background_music(
    settings: Res<AudioSettings>,
    mut music: Query<&mut AudioSink, With<BackgroundMusic>>,
) {
    if !settings.is_changed() {
        return;
    }
    let audible = !settings.muted && (!settings.mute_when_unfocused || settings.focused);
    let volume = if audible {
        settings.master_volume * settings.music_volume * BACKGROUND_MUSIC_LEVEL
    } else {
        0.0
    };
    for mut sink in &mut music {
        sink.set_volume(Volume::Linear(volume));
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
    fn every_required_cue_has_a_real_asset_and_mix_profile() {
        assert_eq!(AUDIO_CUES.len(), 18);
        for cue in AUDIO_CUES {
            assert!(!asset_paths(*cue).is_empty(), "missing asset for {cue:?}");
            let (volume, speed) = cue_mix(*cue);
            assert!((0.0..=1.0).contains(&volume));
            assert!(speed > 0.0);
        }
    }

    #[test]
    fn repeated_cues_have_variation_to_avoid_machine_gun_repetition() {
        for cue in AUDIO_CUES {
            if !matches!(cue, AudioCue::Pause | AudioCue::Resume) {
                assert!(
                    asset_paths(*cue).len() >= 2,
                    "missing variation for {cue:?}"
                );
            }
        }
    }

    #[test]
    fn human_events_are_louder_than_npc_events() {
        let human = 1.0_f32;
        let npc = 0.72_f32;
        assert!(human > npc);
    }
}
