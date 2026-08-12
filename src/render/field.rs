use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};

use super::{
    RetiredMeshes, SKY_COLOR,
    materials::{FlatMaterial, PaperMaterial, RenderAssets},
};

const FIELD_BOTTOM: f32 = -0.34;
const FIELD_DEPTH_OFFSET: Vec2 = Vec2::new(0.22, 0.46);
const FIELD_SHADOW_HEIGHT: f32 = -0.38;
const OUTSIDE_CANVAS_HEIGHT: f32 = -0.46;
const FIELD_EDGE_COLOR: Color = Color::srgb_u8(180, 184, 190);
const FIELD_SIDE_COLOR: Color = Color::srgb_u8(191, 183, 169);

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
pub(super) struct FieldDepth;

#[derive(Component)]
pub(super) struct FieldBorder;

pub(super) fn setup_stage(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FlatMaterial>>,
) {
    commands.spawn((
        Name::new("Outside Canvas"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(260.0, 260.0))),
        MeshMaterial3d(materials.add(FlatMaterial::new(SKY_COLOR, 0.0))),
        Transform::from_xyz(0.0, OUTSIDE_CANVAS_HEIGHT, 0.0),
    ));
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_field_mesh(
    mut commands: Commands,
    field: Res<FieldVisual>,
    render_assets: Option<Res<RenderAssets>>,
    mut retired: ResMut<RetiredMeshes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut flat: ResMut<Assets<FlatMaterial>>,
    surface: Query<(Entity, &Mesh3d), With<FieldSurface>>,
    shadow: Query<(Entity, &Mesh3d), With<FieldShadow>>,
    depth: Query<(Entity, &Mesh3d), With<FieldDepth>>,
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

    let field_mesh = build_field_mesh(&field.contour, 0.0, false);
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

    // Give the paper slab a real side profile. The old top mesh carried a
    // barely visible skirt with the same paper material, so oblique cameras
    // still read the arena as a paper-thin decal. A dedicated warm edge mesh
    // keeps the top surface bright while making the boundary legible.
    let depth_mesh = build_field_depth_mesh(&field.contour, FIELD_BOTTOM, FIELD_DEPTH_OFFSET);
    if let Ok((entity, mesh)) = depth.single() {
        retired.0.push_back((mesh.0.clone(), 0));
        commands
            .entity(entity)
            .insert(Mesh3d(meshes.add(depth_mesh)));
    } else {
        commands.spawn((
            Name::new("Paper Field Edge"),
            FieldDepth,
            Mesh3d(meshes.add(depth_mesh)),
            MeshMaterial3d(flat.add(FlatMaterial::new(FIELD_SIDE_COLOR, 0.72))),
        ));
    }

    let shadow_mesh = build_field_mesh(&field.contour, FIELD_SHADOW_HEIGHT, false);
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
            MeshMaterial3d(flat.add(FlatMaterial::transparent(Color::srgba(
                0.01, 0.02, 0.04, 0.48,
            )))),
            Transform::from_xyz(0.42, 0.0, 0.72),
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
            MeshMaterial3d(flat.add(FlatMaterial::new(FIELD_EDGE_COLOR, 0.0))),
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

fn build_field_depth_mesh(contour: &[Vec2], bottom: f32, depth_offset: Vec2) -> Mesh {
    let mut positions = Vec::with_capacity(contour.len() * 2);
    let mut normals = Vec::with_capacity(contour.len() * 2);
    let mut uvs = Vec::with_capacity(contour.len() * 2);
    let mut indices = Vec::with_capacity(contour.len() * 6);
    for (index, point) in contour.iter().copied().enumerate() {
        let next = contour[(index + 1) % contour.len()];
        let tangent =
            (next - contour[(index + contour.len() - 1) % contour.len()]).normalize_or(Vec2::Y);
        let outward = Vec2::new(tangent.y, -tangent.x);
        let outward = if outward.dot(point) < 0.0 {
            -outward
        } else {
            outward
        };
        positions.push([point.x, 0.0, point.y]);
        let foot = point + depth_offset;
        positions.push([foot.x, bottom, foot.y]);
        normals.extend_from_slice(&[[outward.x, 0.0, outward.y]; 2]);
        uvs.extend_from_slice(&[[index as f32, 0.0], [index as f32, 1.0]]);
    }
    for index in 0..contour.len() {
        let next = (index + 1) % contour.len();
        let a = (index * 2) as u32;
        let b = a + 1;
        let c = (next * 2) as u32;
        let d = c + 1;
        indices.extend_from_slice(&[a, c, b, c, d, b]);
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

    #[test]
    fn field_depth_mesh_has_a_visible_vertical_profile() {
        let contour = vec![
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ];
        let mesh = build_field_depth_mesh(&contour, FIELD_BOTTOM, FIELD_DEPTH_OFFSET);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert_eq!(mesh.count_vertices(), contour.len() * 2);
        assert!(positions.iter().any(|point| point[1] == 0.0));
        assert!(positions.iter().any(|point| point[1] == FIELD_BOTTOM));
        assert!(positions.iter().any(|point| {
            point[1] == FIELD_BOTTOM
                && point[0] == contour[0].x + FIELD_DEPTH_OFFSET.x
                && point[2] == contour[0].y + FIELD_DEPTH_OFFSET.y
        }));
        assert_eq!(mesh.indices().unwrap().len(), contour.len() * 6);
    }
}
