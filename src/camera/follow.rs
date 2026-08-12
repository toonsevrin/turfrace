use crate::render::SKY_COLOR;
use bevy::prelude::*;

/// Render-facing description of a local human who needs a viewport.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ViewportSubject {
    pub slot: u8,
    pub color_id: u8,
    pub position: Vec2,
    pub heading: Vec2,
    pub trail_length: f32,
    /// Normalized 0..1 danger/edge pressure used for subtle awareness zoom.
    pub awareness: f32,
    pub kill_count: u32,
    pub kill_streak: u32,
    pub alive: bool,
}

/// Identifies a camera belonging to one local human.
#[derive(Component, Debug, Clone, Copy)]
pub struct PlayerCamera {
    pub subject: Entity,
    pub slot: u8,
}

#[derive(Resource, Debug, Clone)]
pub struct CameraTuning {
    pub height: f32,
    pub trailing_offset: f32,
    pub look_ahead: f32,
    pub vertical_fov_radians: f32,
    pub follow_half_life: f32,
    pub zoom_half_life: f32,
    pub maximum_zoom_out: f32,
    pub kill_fov_per_kill_radians: f32,
    pub maximum_kill_fov_bonus_radians: f32,
    pub kill_streak_fov_per_kill_radians: f32,
    pub kill_fov_pulse_radians: f32,
}

impl Default for CameraTuning {
    fn default() -> Self {
        Self {
            // Slightly tighter than the raw design-spec reference framing. The
            // old values made cubes read as UI dots in narrow two-player views.
            // Keep cubes readable while the edge-aware zoom supplies context
            // when the player approaches the contour.
            height: 25.4,
            trailing_offset: 10.35,
            look_ahead: 2.5,
            vertical_fov_radians: 48.0_f32.to_radians(),
            follow_half_life: 0.12,
            zoom_half_life: 0.35,
            maximum_zoom_out: 0.28,
            kill_fov_per_kill_radians: 1.35_f32.to_radians(),
            maximum_kill_fov_bonus_radians: 9.0_f32.to_radians(),
            kill_streak_fov_per_kill_radians: 0.55_f32.to_radians(),
            kill_fov_pulse_radians: 3.0_f32.to_radians(),
        }
    }
}

/// A short non-authoritative FOV punch triggered by a credited kill.
#[derive(Component, Debug, Clone, Copy)]
pub struct CameraFovPulse {
    pub remaining: f32,
    pub total: f32,
    pub amount_radians: f32,
}

#[derive(Component)]
pub(super) struct CameraRigState {
    focus: Vec3,
    zoom: f32,
}

#[derive(Component)]
pub(super) struct ViewportDecoration(Entity);

pub(super) fn reconcile_player_cameras(
    mut commands: Commands,
    subjects: Query<(Entity, &ViewportSubject)>,
    cameras: Query<(Entity, &PlayerCamera)>,
    decorations: Query<(Entity, &ViewportDecoration)>,
    tuning: Res<CameraTuning>,
    presentation: Res<crate::render::PresentationSettings>,
) {
    // There are at most eight local players. Linear scans over those tiny
    // queries avoid allocating three hash collections every render frame.
    for (camera_entity, marker) in &cameras {
        if !subjects.iter().any(|(entity, _)| entity == marker.subject) {
            commands.entity(camera_entity).despawn();
        }
    }
    for (entity, decoration) in &decorations {
        if !cameras
            .iter()
            .any(|(camera_entity, _)| camera_entity == decoration.0)
        {
            commands.entity(entity).despawn();
        }
    }

    let local_player_count = subjects.iter().count();
    for (subject_entity, subject) in &subjects {
        if cameras
            .iter()
            .any(|(_, marker)| marker.subject == subject_entity)
        {
            continue;
        }
        let focus = Vec3::new(subject.position.x, 0.0, subject.position.y);
        let camera_entity = commands
            .spawn((
                Name::new(format!("Player {} Camera", subject.slot + 1)),
                Camera3d::default(),
                presentation.gameplay_msaa(local_player_count),
                Projection::Perspective(PerspectiveProjection {
                    fov: tuning.vertical_fov_radians,
                    ..default()
                }),
                Camera {
                    order: subject.slot as isize,
                    clear_color: ClearColorConfig::Custom(SKY_COLOR),
                    ..default()
                },
                Transform::from_translation(
                    focus + Vec3::new(0.0, tuning.height, tuning.trailing_offset),
                )
                .looking_at(focus, Vec3::Y),
                PlayerCamera {
                    subject: subject_entity,
                    slot: subject.slot,
                },
                CameraRigState { focus, zoom: 0.0 },
            ))
            .id();

        commands.spawn((
            Name::new(format!("Player {} Viewport Accent", subject.slot + 1)),
            ViewportDecoration(camera_entity),
            UiTargetCamera(camera_entity),
            Node {
                position_type: PositionType::Absolute,
                top: px(0),
                left: px(0),
                width: percent(100),
                height: px(3),
                ..default()
            },
            BackgroundColor(viewport_accent(subject.color_id)),
            GlobalZIndex(100),
        ));
    }
}

fn viewport_accent(color_id: u8) -> Color {
    crate::palette::palette_color(color_id).with_alpha(0.92)
}

#[allow(clippy::type_complexity)]
pub(super) fn follow_subjects(
    time: Res<Time>,
    tuning: Res<CameraTuning>,
    subjects: Query<&ViewportSubject>,
    mut cameras: Query<(
        &PlayerCamera,
        &Camera,
        &mut CameraRigState,
        &mut Transform,
        &mut Projection,
        Option<&mut CameraFovPulse>,
    )>,
) {
    let dt = time.delta_secs().min(0.1);
    let follow_alpha = 1.0 - 2.0_f32.powf(-dt / tuning.follow_half_life.max(0.001));
    let zoom_alpha = 1.0 - 2.0_f32.powf(-dt / tuning.zoom_half_life.max(0.001));
    for (player_camera, camera, mut rig, mut transform, mut projection, pulse) in &mut cameras {
        let Ok(subject) = subjects.get(player_camera.subject) else {
            continue;
        };
        let heading = subject.heading.normalize_or(Vec2::NEG_Y);
        let desired_focus = Vec3::new(
            subject.position.x + heading.x * tuning.look_ahead,
            0.0,
            subject.position.y + heading.y * tuning.look_ahead,
        );
        rig.focus = rig.focus.lerp(desired_focus, follow_alpha);

        let trail_zoom = (subject.trail_length / 36.0).clamp(0.0, 1.0);
        let dead_zoom = if subject.alive { 0.0 } else { 1.0 };
        let desired_zoom = trail_zoom
            .max(subject.awareness.clamp(0.0, 1.0))
            .max(dead_zoom)
            * tuning.maximum_zoom_out;
        rig.zoom += (desired_zoom - rig.zoom) * zoom_alpha;
        let aspect = camera.viewport.as_ref().map_or(16.0 / 9.0, |viewport| {
            viewport.physical_size.x as f32 / viewport.physical_size.y.max(1) as f32
        });
        // Preserve tactical horizontal awareness in narrow viewports without
        // shrinking the player as aggressively as exact square-root scaling.
        let aspect_compensation = framing_compensation(aspect);
        let scale = (1.0 + rig.zoom) * aspect_compensation;
        let offset = Vec3::new(0.0, tuning.height, tuning.trailing_offset) * scale;
        transform.translation = rig.focus + offset;
        transform.look_at(rig.focus, Vec3::Y);

        let persistent_fov = kill_fov_bonus(subject.kill_count, &tuning);
        let streak_fov =
            subject.kill_streak.min(4) as f32 * tuning.kill_streak_fov_per_kill_radians;
        let pulse_fov = pulse.map_or(0.0, |mut pulse| {
            if pulse.remaining <= 0.0 {
                return 0.0;
            }
            pulse.remaining = (pulse.remaining - dt).max(0.0);
            let progress = 1.0 - pulse.remaining / pulse.total.max(0.001);
            pulse.amount_radians * (progress * std::f32::consts::PI).sin().max(0.0)
        });
        let next_fov = tuning.vertical_fov_radians + persistent_fov + streak_fov + pulse_fov;
        let current_fov = match &*projection {
            Projection::Perspective(perspective) => Some(perspective.fov),
            _ => None,
        };
        if current_fov.is_some_and(|current| (current - next_fov).abs() > 1.0e-5)
            && let Projection::Perspective(perspective) = &mut *projection
        {
            perspective.fov = next_fov;
        }
    }
}

fn framing_compensation(aspect: f32) -> f32 {
    ((16.0 / 9.0) / aspect.max(0.01)).max(1.0).powf(0.44)
}

fn kill_fov_bonus(kills: u32, tuning: &CameraTuning) -> f32 {
    (kills as f32 * tuning.kill_fov_per_kill_radians).min(tuning.maximum_kill_fov_bonus_radians)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_accent_tracks_selected_color_not_slot() {
        assert_eq!(
            viewport_accent(7),
            crate::palette::palette_color(7).with_alpha(0.92)
        );
        assert_ne!(viewport_accent(7), viewport_accent(0));
    }

    #[test]
    fn narrow_viewports_keep_awareness_without_over_shrinking_players() {
        assert_eq!(framing_compensation(16.0 / 9.0), 1.0);
        let side_by_side = framing_compensation(8.0 / 9.0);
        assert!(side_by_side > 1.3 && side_by_side < 1.4);
    }

    #[test]
    fn kill_count_widens_fov_but_respects_the_presentation_cap() {
        let tuning = CameraTuning::default();
        let one_kill = kill_fov_bonus(1, &tuning);
        let many_kills = kill_fov_bonus(100, &tuning);
        assert!(one_kill > 0.0);
        assert_eq!(many_kills, tuning.maximum_kill_fov_bonus_radians);
    }
}
