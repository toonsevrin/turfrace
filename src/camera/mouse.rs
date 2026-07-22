use bevy::{prelude::*, window::PrimaryWindow};

use crate::{
    input::{HumanController, InputDeviceId, MouseAimWorld},
    movement::CompetitorMotion,
};

use super::PlayerCamera;

const MOUSE_DEADZONE_PIXELS: f32 = 24.0;

pub(super) fn update_mouse_aim(
    windows: Query<&Window, With<PrimaryWindow>>,
    mouse_players: Query<(Entity, &HumanController, &CompetitorMotion)>,
    cameras: Query<(&PlayerCamera, &Camera, &GlobalTransform)>,
    settings: Res<crate::profiles::UserSettings>,
    mut mouse_aim: ResMut<MouseAimWorld>,
) {
    mouse_aim.0 = None;
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Some((player_entity, _, motion)) = mouse_players
        .iter()
        .find(|(_, controller, _)| controller.device == InputDeviceId::Mouse)
    else {
        return;
    };
    let Some((_, camera, camera_transform)) = cameras
        .iter()
        .find(|(player_camera, _, _)| player_camera.subject == player_entity)
    else {
        return;
    };

    let clamped_cursor = clamp_cursor_to_viewport(cursor, camera, window.scale_factor());
    let Ok(ray) = camera.viewport_to_world(camera_transform, clamped_cursor) else {
        return;
    };
    let Some(world_point) = intersect_xz_plane(ray.origin, *ray.direction) else {
        return;
    };
    let cube = Vec3::new(motion.position.x, 0.79, motion.position.y);
    let Ok(cube_screen) = camera.world_to_viewport(camera_transform, cube) else {
        return;
    };
    mouse_aim.0 = desired_direction(
        motion.position,
        Vec2::new(world_point.x, world_point.z),
        clamped_cursor.distance(cube_screen),
        MOUSE_DEADZONE_PIXELS / settings.mouse_sensitivity.max(0.01),
    );
}

fn clamp_cursor_to_viewport(cursor: Vec2, camera: &Camera, scale_factor: f32) -> Vec2 {
    let Some(viewport) = &camera.viewport else {
        return cursor;
    };
    let scale = scale_factor.max(0.001);
    let min = viewport.physical_position.as_vec2() / scale;
    let max = (viewport.physical_position + viewport.physical_size).as_vec2() / scale
        - Vec2::splat(0.5 / scale);
    cursor.clamp(min, max)
}

fn intersect_xz_plane(origin: Vec3, direction: Vec3) -> Option<Vec3> {
    if direction.y.abs() <= 1.0e-6 {
        return None;
    }
    let distance = -origin.y / direction.y;
    (distance >= 0.0).then_some(origin + direction * distance)
}

fn desired_direction(
    player: Vec2,
    world_target: Vec2,
    screen_distance: f32,
    deadzone_pixels: f32,
) -> Option<Vec2> {
    if screen_distance < deadzone_pixels {
        return None;
    }
    (world_target - player).try_normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_intersects_gameplay_plane() {
        let hit =
            intersect_xz_plane(Vec3::new(4.0, 10.0, -2.0), Vec3::new(0.2, -1.0, 0.5)).unwrap();
        assert_eq!(hit, Vec3::new(6.0, 0.0, 3.0));
        assert!(intersect_xz_plane(Vec3::Y, Vec3::X).is_none());
    }

    #[test]
    fn screen_deadzone_retains_current_heading() {
        assert_eq!(
            desired_direction(Vec2::ZERO, Vec2::X * 10.0, 23.9, 24.0),
            None
        );
        assert_eq!(
            desired_direction(Vec2::ZERO, Vec2::new(4.0, 3.0), 24.0, 24.0),
            Some(Vec2::new(0.8, 0.6))
        );
    }
}
