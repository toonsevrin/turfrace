use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};

use super::{CompetitorVisual, materials::RenderAssets, territory::TerritoryVisual};

const CAP_SEGMENTS: usize = 10;
const TRAIL_CLEARANCE: f32 = 0.03;

pub const MAX_RENDER_TRAIL_POINTS: usize = 512;

/// Render-only trail polyline. Gameplay collision continues to use its own
/// exact sampled path and head. The visual path has a fixed budget.
#[derive(Component, Debug, Clone, Default)]
pub struct TrailVisual {
    pub points: Vec<Vec2>,
    pub length: f32,
    pub source_samples: usize,
    pub revision: u64,
    pub dangerous: bool,
}

#[derive(Component)]
pub(super) struct TrailProxy {
    source: Entity,
    revision: u64,
}

#[derive(Component)]
pub(super) struct TrailPipelineWarmup;

pub(super) fn spawn_trail_pipeline_warmup(
    mut commands: Commands,
    assets: Res<RenderAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    commands.spawn((
        Name::new("Trail Pipeline Warmup"),
        TrailPipelineWarmup,
        Mesh3d(meshes.add(ribbon_mesh(
            &[Vec2::new(-0.01, 0.0), Vec2::new(0.01, 0.0)],
            0.01,
            &TerritoryVisual::default(),
        ))),
        MeshMaterial3d(assets.trail_materials[0].clone()),
        Transform::from_xyz(0.0, -1.0, 0.0),
    ));
}

pub(super) fn cleanup_trail_pipeline_warmup(
    mut commands: Commands,
    warmups: Query<Entity, With<TrailPipelineWarmup>>,
) {
    for entity in &warmups {
        commands.entity(entity).despawn();
    }
}

pub(super) fn sync_trail_visuals(
    mut commands: Commands,
    assets: Option<Res<RenderAssets>>,
    territory: Res<TerritoryVisual>,
    mut meshes: ResMut<Assets<Mesh>>,
    sources: Query<(Entity, &CompetitorVisual, &TrailVisual)>,
    mut proxies: Query<(Entity, &mut TrailProxy, &Mesh3d)>,
) {
    let Some(assets) = assets else { return };
    let mut rendered = [false; 12];
    for (entity, mut proxy, mesh_handle) in &mut proxies {
        let Some((_, visual, trail)) = sources
            .iter()
            .find(|(source, _, _)| *source == proxy.source)
        else {
            commands.entity(entity).despawn();
            continue;
        };
        rendered[visual.id as usize] = true;
        if proxy.revision != trail.revision {
            if let Some(mut mesh) = meshes.get_mut(&mesh_handle.0) {
                *mesh = ribbon_mesh(&trail.points, 0.65, &territory);
            }
            proxy.revision = trail.revision;
        }
    }
    for (source, visual, trail) in &sources {
        if rendered[visual.id as usize] || trail.points.len() < 2 {
            continue;
        }
        let palette = visual.color_id as usize % assets.trail_materials.len();
        commands.spawn((
            Name::new(format!("Competitor {} Trail", visual.id)),
            TrailProxy {
                source,
                revision: trail.revision,
            },
            Mesh3d(meshes.add(ribbon_mesh(&trail.points, 0.65, &territory))),
            MeshMaterial3d(assets.trail_materials[palette].clone()),
            Transform::default(),
        ));
    }
}

fn ribbon_mesh(points: &[Vec2], width: f32, territory: &TerritoryVisual) -> Mesh {
    let mut positions = Vec::with_capacity(points.len() * 2);
    let mut normals = Vec::with_capacity(points.len() * 2);
    let mut uvs = Vec::with_capacity(points.len() * 2);
    let mut indices = Vec::with_capacity(points.len().saturating_sub(1) * 6);
    let mut distance = 0.0;
    for (index, &point) in points.iter().enumerate() {
        if index > 0 {
            distance += point.distance(points[index - 1]);
        }
        let incoming = if index > 0 {
            (point - points[index - 1]).normalize_or_zero()
        } else {
            (points[1] - point).normalize_or_zero()
        };
        let outgoing = if index + 1 < points.len() {
            (points[index + 1] - point).normalize_or_zero()
        } else {
            incoming
        };
        let normal_a = Vec2::new(-incoming.y, incoming.x);
        let normal_b = Vec2::new(-outgoing.y, outgoing.x);
        let join = (normal_a + normal_b).normalize_or(normal_b);
        let denominator = join.dot(normal_b).abs().max(0.45);
        let offset = join * (width * 0.5 / denominator).min(width * 0.75);
        let height = surface_height(territory, point);
        positions.push([point.x + offset.x, height, point.y + offset.y]);
        positions.push([point.x - offset.x, height, point.y - offset.y]);
        normals.extend_from_slice(&[[0.0, 1.0, 0.0]; 2]);
        uvs.push([distance, 0.0]);
        uvs.push([distance, 1.0]);
        if index + 1 < points.len() {
            let base = (index * 2) as u32;
            indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
        }
    }
    // Round the two free ends instead of leaving a conspicuous square cutoff.
    // This is render-only geometry; the authoritative trail remains untouched.
    let radius = width * 0.5;
    append_round_cap(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        points[0],
        radius,
        surface_height(territory, points[0]),
        0.0,
    );
    append_round_cap(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        *points.last().unwrap_or(&points[0]),
        radius,
        surface_height(territory, *points.last().unwrap_or(&points[0])),
        distance,
    );
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[allow(clippy::too_many_arguments)]
fn append_round_cap(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    center: Vec2,
    radius: f32,
    height: f32,
    along: f32,
) {
    let base = positions.len() as u32;
    positions.push([center.x, height, center.y]);
    normals.push([0.0, 1.0, 0.0]);
    uvs.push([along, 0.5]);
    for segment in 0..CAP_SEGMENTS {
        let angle = std::f32::consts::TAU * segment as f32 / CAP_SEGMENTS as f32;
        positions.push([
            center.x + angle.cos() * radius,
            height,
            center.y + angle.sin() * radius,
        ]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([along, 0.5 + angle.sin() * 0.5]);
    }
    for segment in 0..CAP_SEGMENTS {
        let next = (segment + 1) % CAP_SEGMENTS;
        // Reverse perimeter order so the top face keeps an upward normal.
        indices.extend_from_slice(&[base, base + 1 + next as u32, base + 1 + segment as u32]);
    }
}

fn surface_height(territory: &TerritoryVisual, point: Vec2) -> f32 {
    territory.surface_height(point) + TRAIL_CLEARANCE
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ribbon_uses_two_vertices_per_sample() {
        let mesh = ribbon_mesh(
            &[Vec2::ZERO, Vec2::X, Vec2::new(1.0, 1.0)],
            0.65,
            &TerritoryVisual::default(),
        );
        assert_eq!(mesh.count_vertices(), 28);
        assert_eq!(mesh.indices().unwrap().len(), 72);
    }
    #[test]
    fn acute_join_stays_bounded() {
        let mesh = ribbon_mesh(
            &[Vec2::ZERO, Vec2::X, Vec2::new(0.1, 0.01)],
            0.65,
            &TerritoryVisual::default(),
        );
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert!(
            positions
                .iter()
                .all(|p| p[0].is_finite() && p[2].is_finite())
        );
    }

    #[test]
    fn ribbon_follows_claimed_surface_height() {
        let territory = TerritoryVisual {
            width: 2,
            height: 1,
            cell_size: 1.0,
            owners: vec![0, 1],
            ..default()
        };
        let mesh = ribbon_mesh(
            &[Vec2::new(0.5, 0.5), Vec2::new(1.5, 0.5)],
            0.65,
            &territory,
        );
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert_eq!(
            positions[0][1],
            super::super::territory::FIELD_SURFACE_HEIGHT + TRAIL_CLEARANCE
        );
        assert_eq!(
            positions[2][1],
            super::super::territory::TERRITORY_SURFACE_HEIGHT + TRAIL_CLEARANCE
        );
    }
}
