use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};

use super::{
    RetiredMeshes,
    materials::{PaperMaterial, RenderAssets},
};

/// Render snapshot of the generated star-shaped field contour.
#[derive(Resource, Debug, Clone, Default)]
pub struct FieldVisual {
    pub contour: Vec<Vec2>,
    /// Increment after replacing or deforming the contour.
    pub revision: u64,
}

#[derive(Component)]
pub(super) struct FieldSurface;

#[derive(Component)]
pub(super) struct FieldShadow;

#[derive(Component)]
pub(super) struct FieldBorder;

pub(super) fn setup_stage(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn(AmbientLight {
        color: Color::WHITE,
        brightness: 310.0,
        affects_lightmapped_meshes: true,
    });
    commands.spawn((
        Name::new("Soft Key Light"),
        DirectionalLight {
            illuminance: 3_600.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-20.0, 35.0, 24.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Name::new("Outside Canvas"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(260.0, 260.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb_u8(222, 227, 235),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.055, 0.0),
    ));
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_field_mesh(
    mut commands: Commands,
    field: Res<FieldVisual>,
    render_assets: Option<Res<RenderAssets>>,
    mut retired: ResMut<RetiredMeshes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    surface: Query<(Entity, &Mesh3d), With<FieldSurface>>,
    shadow: Query<(Entity, &Mesh3d), With<FieldShadow>>,
    border: Query<(Entity, &Mesh3d), With<FieldBorder>>,
    mut last_revision: Local<Option<u64>>,
) {
    if last_revision.as_ref() == Some(&field.revision) || field.contour.len() < 3 {
        return;
    }
    let Some(render_assets) = render_assets else {
        return;
    };
    *last_revision = Some(field.revision);

    let field_mesh = build_field_mesh(&field.contour, 0.0, true);
    if let Ok((entity, mesh)) = surface.single() {
        retired.0.push_back((mesh.0.clone(), 0));
        commands
            .entity(entity)
            .insert(Mesh3d(meshes.add(field_mesh)));
    } else {
        commands.spawn((
            Name::new("Paper Field"),
            FieldSurface,
            Mesh3d(meshes.add(field_mesh)),
            MeshMaterial3d::<PaperMaterial>(render_assets.paper_material.clone()),
        ));
    }

    let shadow_mesh = build_field_mesh(&field.contour, -0.035, false);
    if let Ok((entity, mesh)) = shadow.single() {
        retired.0.push_back((mesh.0.clone(), 0));
        commands
            .entity(entity)
            .insert(Mesh3d(meshes.add(shadow_mesh)));
    } else {
        commands.spawn((
            Name::new("Field Drop Shadow"),
            FieldShadow,
            Mesh3d(meshes.add(shadow_mesh)),
            MeshMaterial3d(standard.add(StandardMaterial {
                base_color: Color::srgba(0.10, 0.12, 0.16, 0.12),
                alpha_mode: AlphaMode::Premultiplied,
                unlit: true,
                ..default()
            })),
            Transform::from_xyz(0.28, 0.0, 0.38),
        ));
    }

    let border_mesh = build_border_mesh(&field.contour, 0.10);
    if let Ok((entity, mesh)) = border.single() {
        retired.0.push_back((mesh.0.clone(), 0));
        commands
            .entity(entity)
            .insert(Mesh3d(meshes.add(border_mesh)));
    } else {
        commands.spawn((
            Name::new("Field Contour Line"),
            FieldBorder,
            Mesh3d(meshes.add(border_mesh)),
            MeshMaterial3d(standard.add(StandardMaterial {
                base_color: Color::srgb_u8(170, 178, 191),
                unlit: true,
                ..default()
            })),
        ));
    }
}

fn build_border_mesh(contour: &[Vec2], width: f32) -> Mesh {
    let mut positions = Vec::with_capacity(contour.len() * 2);
    let mut normals = Vec::with_capacity(contour.len() * 2);
    let mut uvs = Vec::with_capacity(contour.len() * 2);
    let mut indices = Vec::with_capacity(contour.len() * 6);
    for (index, point) in contour.iter().copied().enumerate() {
        let previous = contour[(index + contour.len() - 1) % contour.len()];
        let next = contour[(index + 1) % contour.len()];
        let tangent = (next - previous).normalize_or(Vec2::Y);
        let outward = Vec2::new(tangent.y, -tangent.x);
        let outward = if outward.dot(point) < 0.0 {
            -outward
        } else {
            outward
        };
        positions.push([point.x, 0.012, point.y]);
        let inner = point - outward * width;
        positions.push([inner.x, 0.012, inner.y]);
        normals.extend_from_slice(&[[0.0, 1.0, 0.0]; 2]);
        uvs.extend_from_slice(&[[index as f32, 0.0], [index as f32, 1.0]]);
        let next_index = (index + 1) % contour.len();
        let outer = (index * 2) as u32;
        let inner = outer + 1;
        let next_outer = (next_index * 2) as u32;
        let next_inner = next_outer + 1;
        indices.extend_from_slice(&[outer, next_outer, inner, inner, next_outer, next_inner]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn build_field_mesh(contour: &[Vec2], height: f32, include_skirt: bool) -> Mesh {
    let mut positions = vec![[0.0, height, 0.0]];
    let mut normals = vec![[0.0, 1.0, 0.0]];
    let mut uvs = vec![[0.5, 0.5]];
    let radius = contour.iter().map(|p| p.length()).fold(1.0_f32, f32::max);
    for point in contour {
        positions.push([point.x, height, point.y]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([
            point.x / (radius * 2.0) + 0.5,
            point.y / (radius * 2.0) + 0.5,
        ]);
    }
    let mut indices = Vec::with_capacity(contour.len() * if include_skirt { 9 } else { 3 });
    for index in 0..contour.len() {
        indices.extend_from_slice(&[
            0,
            ((index + 1) % contour.len() + 1) as u32,
            (index + 1) as u32,
        ]);
    }
    if include_skirt {
        let top_start = positions.len();
        for point in contour {
            positions.push([point.x, height, point.y]);
            positions.push([point.x, height - 0.10, point.y]);
            let normal = Vec3::new(point.x, 0.0, point.y).normalize_or_zero();
            normals.extend_from_slice(&[[normal.x, 0.0, normal.z]; 2]);
            uvs.extend_from_slice(&[[0.0, 0.0], [0.0, 1.0]]);
        }
        for index in 0..contour.len() {
            let next = (index + 1) % contour.len();
            let a = (top_start + index * 2) as u32;
            let b = a + 1;
            let c = (top_start + next * 2) as u32;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, c, d, b]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_mesh_is_a_fan_with_skirt() {
        let contour = vec![
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ];
        let mesh = build_field_mesh(&contour, 0.0, true);
        assert_eq!(mesh.count_vertices(), 13);
        assert_eq!(mesh.indices().unwrap().len(), 36);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let indices: Vec<_> = mesh.indices().unwrap().iter().take(3).collect();
        let a = Vec3::from(positions[indices[0] as usize]);
        let b = Vec3::from(positions[indices[1] as usize]);
        let c = Vec3::from(positions[indices[2] as usize]);
        assert!(
            (b - a).cross(c - a).y > 0.0,
            "field top must face cameras above"
        );
    }
}
