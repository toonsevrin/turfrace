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
                simulation_events::bridge_simulation_events,
            ),
        );
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
