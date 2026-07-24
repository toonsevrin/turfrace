//! One-stop installation point for all non-authoritative game presentation.

use bevy::prelude::*;

use crate::{
    audio::GameAudioPlugin, camera::SplitScreenPlugin, effects::EffectsPlugin, render::RenderPlugin,
};

mod simulation_events;

pub struct PresentationPlugin;

impl Plugin for PresentationPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            RenderPlugin,
            SplitScreenPlugin,
            EffectsPlugin,
            GameAudioPlugin,
        ));
        app.add_systems(
            Update,
            (
                sync_user_settings,
                sync_camera_quality,
                simulation_events::bridge_simulation_events,
            ),
        );
    }
}

fn sync_camera_quality(
    settings: Res<crate::render::PresentationSettings>,
    mut cameras: Query<(Ref<Camera>, Option<&crate::camera::PlayerCamera>, &mut Msaa)>,
) {
    let settings_changed = settings.is_changed();
    let local_player_count = cameras
        .iter_mut()
        .filter(|(_, player, _)| player.is_some())
        .count();
    for (camera, player, mut msaa) in &mut cameras {
        // Every camera composited into the window must agree with the gameplay
        // attachments or the global UI can be submitted once per split view
        // on WebGL2-compatible backends.
        let samples = if player.is_some() || local_player_count > 0 {
            settings.gameplay_msaa(local_player_count)
        } else {
            settings.msaa()
        };
        if settings_changed || camera.is_added() || *msaa != samples {
            *msaa = samples;
        }
    }
}

fn sync_user_settings(
    settings: Res<crate::profiles::UserSettings>,
    mut presentation: ResMut<crate::render::PresentationSettings>,
    mut audio: ResMut<crate::audio::AudioSettings>,
) {
    if !settings.is_changed() {
        return;
    }
    presentation.territory_patterns = settings.colorblind_assist;
    presentation.reduced_motion = settings.reduced_motion;
    presentation.camera_shake = settings.screen_shake;
    presentation.quality = match settings.graphics_quality {
        crate::profiles::GraphicsQualitySetting::Low => crate::render::GraphicsQuality::Low,
        crate::profiles::GraphicsQualitySetting::Medium => crate::render::GraphicsQuality::Medium,
        crate::profiles::GraphicsQualitySetting::High => crate::render::GraphicsQuality::High,
    };
    audio.master_volume = settings.master_volume;
    audio.music_volume = settings.music_volume;
    audio.effects_volume = settings.sound_effect_volume;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_samples_scale_with_presentation_quality() {
        assert_eq!(
            crate::render::PresentationSettings {
                quality: crate::render::GraphicsQuality::Low,
                ..default()
            }
            .msaa(),
            Msaa::Off
        );
        assert_eq!(
            crate::render::PresentationSettings::default().msaa(),
            Msaa::Off
        );
        assert_eq!(
            crate::render::PresentationSettings {
                quality: crate::render::GraphicsQuality::High,
                ..default()
            }
            .msaa(),
            Msaa::Sample4
        );
        assert_eq!(
            crate::render::PresentationSettings::default().gameplay_msaa(1),
            Msaa::Off
        );
        assert_eq!(
            crate::render::PresentationSettings::default().gameplay_msaa(2),
            Msaa::Sample4
        );
    }
}
