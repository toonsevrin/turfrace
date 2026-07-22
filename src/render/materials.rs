use bevy::{
    asset::Asset,
    pbr::Material,
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, Face},
    shader::ShaderRef,
};

use super::{PLAYER_COLORS, PresentationSettings, mix_with_white};

const TRAIL_SHADER: &str = "shaders/trail.wgsl";
const TERRITORY_SHADER: &str = "shaders/territory.wgsl";
const PAPER_SHADER: &str = "shaders/paper.wgsl";

pub(super) struct PresentationMaterialPlugin;

impl Plugin for PresentationMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<TrailMaterial>::default(),
            MaterialPlugin::<TerritoryMaterial>::default(),
            MaterialPlugin::<PaperMaterial>::default(),
        ));
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct TrailMaterial {
    #[uniform(0)]
    pub color: LinearRgba,
    #[uniform(0)]
    pub parameters: Vec4,
}

impl Material for TrailMaterial {
    fn fragment_shader() -> ShaderRef {
        TRAIL_SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct TerritoryMaterial {
    #[uniform(0)]
    pub parameters: Vec4,
}

impl Material for TerritoryMaterial {
    fn fragment_shader() -> ShaderRef {
        TERRITORY_SHADER.into()
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PaperMaterial {
    #[uniform(0)]
    pub color: LinearRgba,
    #[uniform(0)]
    pub parameters: Vec4,
}

impl Material for PaperMaterial {
    fn fragment_shader() -> ShaderRef {
        PAPER_SHADER.into()
    }
}

#[derive(Resource)]
pub(super) struct RenderAssets {
    pub cube_mesh: Handle<Mesh>,
    pub inner_cube_mesh: Handle<Mesh>,
    pub shadow_mesh: Handle<Mesh>,
    pub icon_mesh: Handle<Mesh>,
    pub ring_mesh: Handle<Mesh>,
    pub crown_mesh: Handle<Mesh>,
    pub black_outline: Handle<StandardMaterial>,
    pub charcoal: Handle<StandardMaterial>,
    pub shadow: Handle<StandardMaterial>,
    pub cube_materials: Vec<Handle<StandardMaterial>>,
    pub accent_materials: Vec<Handle<StandardMaterial>>,
    pub trail_materials: Vec<Handle<TrailMaterial>>,
    pub territory_material: Handle<TerritoryMaterial>,
    pub paper_material: Handle<PaperMaterial>,
}

pub(super) fn setup_render_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut trails: ResMut<Assets<TrailMaterial>>,
    mut territories: ResMut<Assets<TerritoryMaterial>>,
    mut papers: ResMut<Assets<PaperMaterial>>,
) {
    let cube_mesh = meshes.add(Cuboid::from_size(Vec3::splat(1.50)));
    let inner_cube_mesh = meshes.add(Cuboid::from_size(Vec3::splat(1.30)));
    let shadow_mesh = meshes.add(Circle::new(0.82));
    let icon_mesh = meshes.add(Circle::new(0.26));
    let ring_mesh = meshes.add(Torus::new(0.76, 0.055));
    let crown_mesh = meshes.add(Cone::new(0.24, 0.48));
    let black_outline = standard.add(StandardMaterial {
        base_color: Color::srgb_u8(23, 25, 29),
        unlit: true,
        cull_mode: Some(Face::Front),
        ..default()
    });
    let charcoal = standard.add(StandardMaterial {
        base_color: Color::srgb_u8(23, 25, 29),
        perceptual_roughness: 0.92,
        ..default()
    });
    let shadow = standard.add(StandardMaterial {
        base_color: Color::srgba(0.06, 0.07, 0.09, 0.18),
        alpha_mode: AlphaMode::Premultiplied,
        unlit: true,
        ..default()
    });

    let mut cube_materials = Vec::with_capacity(PLAYER_COLORS.len());
    let mut accent_materials = Vec::with_capacity(PLAYER_COLORS.len());
    let mut trail_materials = Vec::with_capacity(PLAYER_COLORS.len());
    for color in PLAYER_COLORS {
        let base = Color::Srgba(color);
        cube_materials.push(standard.add(StandardMaterial {
            base_color: mix_with_white(base, 0.16),
            perceptual_roughness: 0.74,
            reflectance: 0.20,
            ..default()
        }));
        accent_materials.push(standard.add(StandardMaterial {
            base_color: mix_with_white(base, 0.08),
            emissive: LinearRgba::from(base) * 0.12,
            ..default()
        }));
        let linear = LinearRgba::from(base);
        trail_materials.push(trails.add(TrailMaterial {
            // Premultiplied blending used to make trails look like washed-out
            // string: the colour was halved before the shader halved it again.
            color: LinearRgba::new(
                linear.red * 0.96,
                linear.green * 0.96,
                linear.blue * 0.96,
                0.78,
            ),
            parameters: Vec4::new(0.0, 1.0, 0.0, 0.0),
        }));
    }

    commands.insert_resource(RenderAssets {
        cube_mesh,
        inner_cube_mesh,
        shadow_mesh,
        icon_mesh,
        ring_mesh,
        crown_mesh,
        black_outline,
        charcoal,
        shadow,
        cube_materials,
        accent_materials,
        trail_materials,
        territory_material: territories.add(TerritoryMaterial {
            parameters: Vec4::new(1.0, 0.0, 0.0, 0.0),
        }),
        paper_material: papers.add(PaperMaterial {
            // Warm paper keeps the arena from reading as a blank white debug
            // canvas and gives the player colors a stable high-contrast field.
            color: LinearRgba::new(0.91, 0.875, 0.79, 1.0),
            parameters: Vec4::new(0.035, 0.0, 0.0, 0.0),
        }),
    });
}

/// Synchronize the small set of material switches only when presentation
/// settings actually change.  Mutating custom-material assets every frame can
/// force WebGL2 uniform re-preparation and, on some drivers, trips the slab
/// allocator while an old bind group is still in flight.  Motion is generated
/// in the shader from stable world coordinates instead.
pub(super) fn sync_material_settings(
    settings: Res<PresentationSettings>,
    assets: Option<Res<RenderAssets>>,
    mut trails: ResMut<Assets<TrailMaterial>>,
    mut territories: ResMut<Assets<TerritoryMaterial>>,
) {
    if !settings.is_changed() {
        return;
    }
    let Some(assets) = assets else { return };
    for handle in &assets.trail_materials {
        if let Some(mut material) = trails.get_mut(handle) {
            material.parameters.y = if settings.reduced_motion { 0.0 } else { 1.0 };
        }
    }
    if let Some(mut material) = territories.get_mut(&assets.territory_material) {
        material.parameters.x = f32::from(settings.territory_patterns);
    }
}
