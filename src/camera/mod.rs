//! Split-screen viewport management and smooth, north-locked player cameras.

mod follow;
mod layout;
mod mouse;

pub use follow::{CameraFovPulse, CameraTuning, PlayerCamera, ViewportSubject};
#[allow(unused_imports)]
pub use layout::{ViewportRect, viewport_layout};

use bevy::prelude::*;

/// Installs camera reconciliation, resize handling, and presentation-time following.
pub struct SplitScreenPlugin;

impl Plugin for SplitScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraTuning>()
            .add_systems(
                Update,
                (
                    follow::reconcile_player_cameras,
                    layout::update_camera_viewports,
                    follow::follow_subjects,
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                mouse::update_mouse_aim.after(TransformSystems::Propagate),
            );
    }
}
