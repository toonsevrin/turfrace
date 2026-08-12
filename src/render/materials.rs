use bevy::{
    asset::Asset,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{Material, MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

use super::{PLAYER_COLORS, mix_with_white};

const TERRITORY_SHADER: &str = "shaders/territory.wgsl";
const PAPER_SHADER: &str = "shaders/paper.wgsl";
const FLAT_SHADER: &str = "shaders/flat.wgsl";

pub(super) struct PresentationMaterialPlugin;

impl Plugin for PresentationMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<TerritoryMaterial>::default(),
            MaterialPlugin::<PaperMaterial>::default(),
            MaterialPlugin::<FlatMaterial>::default(),
        ));
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(FlatMaterialKey)]
pub struct FlatMaterial {
    #[uniform(0)]
    pub color: LinearRgba,
    #[uniform(0)]
    pub parameters: Vec4,
    pub alpha_mode: AlphaMode,
    pub cull_front: bool,
}

impl FlatMaterial {
    pub fn new(color: Color, lighting: f32) -> Self {
        Self {
            color: color.into(),
            parameters: Vec4::new(lighting.clamp(0.0, 1.0), 0.0, 0.0, 0.0),
            alpha_mode: AlphaMode::Opaque,
            cull_front: false,
        }
    }

    pub fn transparent(color: Color) -> Self {
        Self {
            color: color.into(),
            parameters: Vec4::ZERO,
            alpha_mode: AlphaMode::Premultiplied,
            cull_front: false,
        }
    }
}

#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct FlatMaterialKey {
    cull_front: bool,
}

impl From<&FlatMaterial> for FlatMaterialKey {
    fn from(material: &FlatMaterial) -> Self {
        Self {
            cull_front: material.cull_front,
        }
    }
}

impl Material for FlatMaterial {
    fn fragment_shader() -> ShaderRef {
        FLAT_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = Some(if key.bind_group_data.cull_front {
            Face::Front
        } else {
            Face::Back
        });
        Ok(())
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct TerritoryMaterial {
    #[uniform(0)]
    pub color: LinearRgba,
    #[uniform(0)]
    pub parameters: Vec4,
}

impl Material for TerritoryMaterial {
    fn fragment_shader() -> ShaderRef {
        TERRITORY_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Opaque
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Owner loops can be outer contours or holes. Rendering both winding
        // directions keeps the capture-time triangulation robust at seams.
        descriptor.primitive.cull_mode = None;
        Ok(())
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
    pub black_outline: Handle<FlatMaterial>,
    pub charcoal: Handle<FlatMaterial>,
    pub shadow: Handle<FlatMaterial>,
    pub cube_materials: Vec<Handle<FlatMaterial>>,
    pub accent_materials: Vec<Handle<FlatMaterial>>,
    pub trail_materials: Vec<Handle<FlatMaterial>>,
    pub paper_material: Handle<PaperMaterial>,
}

pub(super) fn setup_render_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut flat: ResMut<Assets<FlatMaterial>>,
    mut papers: ResMut<Assets<PaperMaterial>>,
) {
    let cube_mesh = meshes.add(Cuboid::from_size(Vec3::splat(1.50)));
    let inner_cube_mesh = meshes.add(Cuboid::from_size(Vec3::splat(1.30)));
    let shadow_mesh = meshes.add(Circle::new(0.82));
    let icon_mesh = meshes.add(Circle::new(0.26));
    let ring_mesh = meshes.add(Torus::new(0.76, 0.055));
    let crown_mesh = meshes.add(Cone::new(0.24, 0.48));
    let black_outline = flat.add(FlatMaterial {
        cull_front: true,
        ..FlatMaterial::new(Color::srgb_u8(23, 25, 29), 0.0)
    });
    let charcoal = flat.add(FlatMaterial::new(Color::srgb_u8(23, 25, 29), 0.28));
    let shadow = flat.add(FlatMaterial::transparent(Color::srgba(
        0.06, 0.07, 0.09, 0.18,
    )));

    let mut cube_materials = Vec::with_capacity(PLAYER_COLORS.len());
    let mut accent_materials = Vec::with_capacity(PLAYER_COLORS.len());
    let mut trail_materials = Vec::with_capacity(PLAYER_COLORS.len());
    for color in PLAYER_COLORS {
        let base = Color::Srgba(color);
        cube_materials.push(flat.add(FlatMaterial::new(mix_with_white(base, 0.16), 0.32)));
        accent_materials.push(flat.add(FlatMaterial::new(mix_with_white(base, 0.08), 0.10)));
        trail_materials.push(flat.add(FlatMaterial::transparent(
            mix_with_white(base, 0.08).with_alpha(0.72),
        )));
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
        paper_material: papers.add(PaperMaterial {
            // Near-white paper gives the field a deliberate surface while the
            // neutral outside canvas and edge profile provide the depth cue.
            color: LinearRgba::new(0.965, 0.958, 0.932, 1.0),
            parameters: Vec4::new(0.16, 0.0, 0.0, 0.0),
        }),
    });
}
