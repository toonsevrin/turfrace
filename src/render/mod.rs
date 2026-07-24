//! Decoupled 3D presentation for the authoritative two-dimensional simulation.
//!
//! Gameplay code updates [`FieldVisual`], [`TerritoryVisual`], and the visual
//! components on competitors. Rendering never mutates simulation state.

mod cube;
mod field;
mod materials;
mod sync;
mod territory;
mod trail;

pub use cube::CompetitorVisual;
pub use field::FieldVisual;
pub use materials::FlatMaterial;
pub use territory::TerritoryVisual;
pub use trail::TrailVisual;

use bevy::prelude::*;
use std::collections::VecDeque;

use crate::palette::PLAYER_COLORS;

pub(crate) fn mix_with_white(color: Color, amount: f32) -> Color {
    let c = color.to_srgba();
    Color::srgb(
        c.red + (1.0 - c.red) * amount,
        c.green + (1.0 - c.green) * amount,
        c.blue + (1.0 - c.blue) * amount,
    )
}

#[derive(Resource, Debug, Clone)]
pub struct PresentationSettings {
    pub territory_patterns: bool,
    pub reduced_motion: bool,
    pub camera_shake: f32,
    pub quality: GraphicsQuality,
}

impl Default for PresentationSettings {
    fn default() -> Self {
        Self {
            territory_patterns: true,
            reduced_motion: false,
            camera_shake: 0.65,
            quality: GraphicsQuality::Medium,
        }
    }
}

impl PresentationSettings {
    pub(crate) fn msaa(&self) -> Msaa {
        match self.quality {
            GraphicsQuality::Low | GraphicsQuality::Medium => Msaa::Off,
            GraphicsQuality::High => Msaa::Sample4,
        }
    }

    pub(crate) fn gameplay_msaa(&self, local_player_count: usize) -> Msaa {
        if local_player_count > 1 {
            // WebGL2-compatible wgpu backends can lose earlier split views
            // when several window cameras render without a multisampled
            // attachment. Four samples are guaranteed by WebGPU and preserve
            // every local viewport.
            Msaa::Sample4
        } else {
            self.msaa()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsQuality {
    Low,
    Medium,
    High,
}

/// Mesh handles are held briefly after a component replacement so the render
/// world can finish copying the previous allocation.  This is especially
/// important on WebGL2 where the mesh allocator uses one buffer per vertex
/// array and extraction can lag the main world by a frame.
#[derive(Resource, Default)]
pub(super) struct RetiredMeshes(VecDeque<(Handle<Mesh>, u8)>);

fn retire_meshes(mut retired: ResMut<RetiredMeshes>) {
    for (_, age) in &mut retired.0 {
        *age = age.saturating_add(1);
    }
    retired.0.retain(|(_, age)| *age < 3);
}

/// Installs field, chunked territory, ribbon, cube, and material presentation.
pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PresentationSettings>()
            .init_resource::<FieldVisual>()
            .init_resource::<TerritoryVisual>()
            .init_resource::<RetiredMeshes>()
            .add_plugins(materials::PresentationMaterialPlugin)
            .add_systems(
                Startup,
                (materials::setup_render_assets, field::setup_stage).chain(),
            )
            .add_systems(
                OnEnter(crate::app_state::AppState::MatchLoading),
                trail::spawn_trail_pipeline_warmup,
            )
            .add_systems(
                OnExit(crate::app_state::AppState::MatchLoading),
                trail::cleanup_trail_pipeline_warmup,
            )
            .add_systems(
                Update,
                (
                    sync::sync_board_visuals,
                    sync::sync_competitor_snapshots,
                    field::sync_field_mesh,
                    territory::sync_territory_surface,
                    cube::sync_competitor_visuals,
                    trail::sync_trail_visuals,
                )
                    .chain(),
            );
        app.add_systems(PostUpdate, retire_meshes);
    }
}
