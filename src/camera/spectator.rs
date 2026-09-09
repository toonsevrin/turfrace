use bevy::{camera::Viewport, prelude::*, window::PrimaryWindow};

use crate::{
    app_state::AppState,
    match_game::{MatchPhase, MatchPurpose, MatchSession},
    render::{PresentationSettings, SKY_COLOR},
    territory_map::TerritoryMap,
};

const HOME_COVER_MARGIN: f32 = 0.90;

/// Identifies the full-field camera used by the Home attract match.
#[derive(Component, Debug, Clone, Copy)]
pub struct SpectatorCamera;

pub(super) fn reconcile_spectator_camera(
    mut commands: Commands,
    state: Res<State<AppState>>,
    session: Res<MatchSession>,
    cameras: Query<Entity, With<SpectatorCamera>>,
    player_cameras: Query<(), With<super::PlayerCamera>>,
    presentation: Res<PresentationSettings>,
    tuning: Res<super::CameraTuning>,
) {
    let should_exist = (state.get().shows_attract_match()
        && session.purpose == MatchPurpose::Attract
        && session.phase != MatchPhase::Idle)
        || (matches!(*state.get(), AppState::Countdown | AppState::Playing)
            && session.purpose == MatchPurpose::Playable
            && session.phase != MatchPhase::Idle
            && player_cameras.is_empty());
    if should_exist && cameras.is_empty() {
        commands.spawn((
            Name::new("Full Field Spectator Camera"),
            SpectatorCamera,
            Camera3d::default(),
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            presentation.msaa(),
            Projection::Perspective(PerspectiveProjection {
                fov: tuning.vertical_fov_radians,
                ..default()
            }),
            Camera {
                order: 0,
                clear_color: ClearColorConfig::Custom(SKY_COLOR),
                ..default()
            },
            Transform::default(),
        ));
    } else if !should_exist {
        for entity in &cameras {
            commands.entity(entity).despawn();
        }
    }
}

pub(super) fn fit_spectator_camera(
    windows: Query<&Window, With<PrimaryWindow>>,
    territory: Res<TerritoryMap>,
    tuning: Res<super::CameraTuning>,
    mut cameras: Query<(&mut Camera, &mut Transform, &mut Projection), With<SpectatorCamera>>,
) {
    let Ok(window) = windows.single() else { return };
    let Some((min, max)) = territory.arena().bounds() else {
        return;
    };
    let min = min.world();
    let max = max.world();
    let center = (min + max) * 0.5;
    let half_extents = (max - min) * 0.5;
    let viewport = home_field_viewport(window.physical_size());
    let aspect = viewport.physical_size.x as f32 / viewport.physical_size.y.max(1) as f32;
    let direction = Vec3::new(0.0, tuning.height, tuning.trailing_offset).normalize_or(Vec3::Y);
    let distance = cover_distance(half_extents, direction, aspect, tuning.vertical_fov_radians);
    let focus = Vec3::new(center.x, 0.0, center.y);
    for (mut camera, mut transform, mut projection) in &mut cameras {
        let viewport_changed = camera.viewport.as_ref().is_none_or(|current| {
            current.physical_position != viewport.physical_position
                || current.physical_size != viewport.physical_size
        });
        if viewport_changed {
            camera.viewport = Some(viewport.clone());
        }
        transform.translation = focus + direction * distance;
        transform.look_at(focus, Vec3::Y);
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.fov = tuning.vertical_fov_radians;
        }
    }
}

fn home_field_viewport(window_size: UVec2) -> Viewport {
    Viewport {
        physical_position: UVec2::ZERO,
        physical_size: window_size.max(UVec2::ONE),
        ..default()
    }
}

fn cover_distance(half_extents: Vec2, direction: Vec3, aspect: f32, vertical_fov: f32) -> f32 {
    let vertical_half_fov = (vertical_fov * 0.5).clamp(0.01, 1.5);
    let horizontal_half_fov = (vertical_half_fov.tan() * aspect.max(0.01)).atan();
    let forward = -direction;
    let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = right.cross(forward).normalize_or(Vec3::Y);
    let extents = Vec3::new(half_extents.x, 0.0, half_extents.y);
    let horizontal = extents.dot(right).abs();
    let vertical = extents.dot(up).abs();
    let depth = extents.dot(forward).abs();
    let horizontal_distance = horizontal / horizontal_half_fov.tan().max(0.01);
    let vertical_distance = vertical / vertical_half_fov.tan().max(0.01);
    (horizontal_distance.min(vertical_distance) + depth + 2.0) * HOME_COVER_MARGIN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_distance_adapts_to_the_viewport_aspect() {
        let half_extents = Vec2::new(50.0, 50.0);
        let direction = Vec3::new(0.0, 25.4, 10.35).normalize();
        let wide = cover_distance(half_extents, direction, 16.0 / 9.0, 48.0_f32.to_radians());
        let narrow = cover_distance(half_extents, direction, 9.0 / 16.0, 48.0_f32.to_radians());
        assert!(narrow > wide);
    }

    #[test]
    fn cover_distance_scales_with_arena_radius() {
        let direction = Vec3::new(0.0, 25.4, 10.35).normalize();
        let small = cover_distance(
            Vec2::splat(10.0),
            direction,
            16.0 / 9.0,
            48.0_f32.to_radians(),
        );
        let large = cover_distance(
            Vec2::splat(20.0),
            direction,
            16.0 / 9.0,
            48.0_f32.to_radians(),
        );
        assert!(large > small * 1.8);
        assert!(small > 10.0);
    }

    #[test]
    fn shell_states_keep_the_attract_field_behind_their_ui() {
        for state in [
            AppState::Home,
            AppState::Lobby,
            AppState::LocalLeaderboard,
            AppState::Settings,
        ] {
            assert!(state.shows_attract_match());
        }
        for state in [
            AppState::Boot,
            AppState::MatchLoading,
            AppState::Countdown,
            AppState::Playing,
            AppState::Paused,
            AppState::GameOver,
            AppState::Results,
        ] {
            assert!(!state.shows_attract_match());
        }
    }

    #[test]
    fn home_field_viewport_fills_the_window_behind_centered_navigation() {
        let viewport = home_field_viewport(UVec2::new(1_280, 720));
        assert_eq!(viewport.physical_position, UVec2::ZERO);
        assert_eq!(viewport.physical_size, UVec2::new(1_280, 720));
    }

    #[test]
    fn home_field_viewport_stays_valid_for_zero_sized_windows() {
        let viewport = home_field_viewport(UVec2::ZERO);
        assert_eq!(viewport.physical_position, UVec2::ZERO);
        assert_eq!(viewport.physical_size, UVec2::ONE);
    }
}
