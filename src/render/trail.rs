use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};

use super::{CompetitorVisual, materials::RenderAssets, territory::TERRITORY_SURFACE_HEIGHT};

const CAP_SEGMENTS: usize = 7;
const MITER_LIMIT: f32 = 1.65;
const POINT_EPSILON_SQUARED: f32 = 1.0e-6;
const TRAIL_CLEARANCE: f32 = 0.03;
const TRAIL_TRAVERSAL_HEIGHT: f32 = TERRITORY_SURFACE_HEIGHT + TRAIL_CLEARANCE;

/// Trails can remain authoritative at full resolution, but their cosmetic
/// ribbon never needs hundreds of vertices on an old browser GPU. A bounded
/// 256-point sample also keeps capture-time mesh uploads predictable.
pub const MAX_RENDER_TRAIL_POINTS: usize = 256;

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
                *mesh = ribbon_mesh(&trail.points, 0.65);
            }
            proxy.revision = trail.revision;
        }
    }
    for (source, visual, trail) in &sources {
        if rendered[visual.id as usize] || !trail_has_ribbon_geometry(trail) {
            continue;
        }
        let palette = visual.color_id as usize % assets.trail_materials.len();
        commands.spawn((
            Name::new(format!("Competitor {} Trail", visual.id)),
            TrailProxy {
                source,
                revision: trail.revision,
            },
            Mesh3d(meshes.add(ribbon_mesh(&trail.points, 0.65))),
            MeshMaterial3d(assets.trail_materials[palette].clone()),
            Transform::default(),
        ));
    }
}

fn trail_has_ribbon_geometry(trail: &TrailVisual) -> bool {
    trail.points.len() >= 2
}

fn ribbon_mesh(points: &[Vec2], width: f32) -> Mesh {
    // The simulation path is intentionally not the render path. Removing
    // duplicate samples here prevents zero-length normals and makes the
    // tessellator robust to a stopped or reversed heading.
    let mut path = Vec::with_capacity(points.len());
    for &point in points {
        if path
            .last()
            .is_none_or(|last: &Vec2| last.distance_squared(point) > POINT_EPSILON_SQUARED)
        {
            path.push(point);
        }
    }

    let mut buffers = RibbonBuffers::with_capacity(path.len());
    if path.len() < 2 {
        return buffers.into_mesh();
    }

    let radius = width.max(0.001) * 0.5;
    let mut distances = vec![0.0; path.len()];
    for index in 1..path.len() {
        distances[index] = distances[index - 1] + path[index].distance(path[index - 1]);
    }

    let directions: Vec<_> = path
        .windows(2)
        .map(|segment| (segment[1] - segment[0]).normalize())
        .collect();
    let mut joins = Vec::with_capacity(path.len());
    joins.push(RibbonJoin::endpoint(path[0], directions[0], radius));
    for index in 1..path.len() - 1 {
        joins.push(RibbonJoin::between(
            path[index],
            directions[index - 1],
            directions[index],
            radius,
        ));
    }
    joins.push(RibbonJoin::endpoint(
        path[path.len() - 1],
        directions[directions.len() - 1],
        radius,
    ));

    // Each segment owns a non-intersecting quad. Ordinary miters still share
    // the same positions, while bevels and reversals may use distinct incoming
    // and outgoing pairs without forcing either segment to cross itself.
    for index in 0..directions.len() {
        let base = buffers.positions.len() as u32;
        let start = joins[index].outgoing;
        let end = joins[index + 1].incoming;
        buffers.push_vertex(start[0], distances[index], 0.0);
        buffers.push_vertex(start[1], distances[index], 1.0);
        buffers.push_vertex(end[0], distances[index + 1], 0.0);
        buffers.push_vertex(end[1], distances[index + 1], 1.0);
        buffers.indices.extend_from_slice(&[
            base,
            base + 2,
            base + 1,
            base + 1,
            base + 2,
            base + 3,
        ]);
    }
    for (index, join) in joins.iter().enumerate().skip(1).take(path.len() - 2) {
        if let Some([start, end]) = join.bevel {
            buffers.triangle(path[index], start, end, distances[index]);
        }
    }

    // The trail emerges from owned territory with a butt cap. At the live head
    // only the forward-facing semicircle is added; the cube covers that half,
    // while no full disk can inflate a tight turn behind it.
    append_round_cap(
        &mut buffers,
        path[path.len() - 2],
        path[path.len() - 1],
        radius,
        distances[path.len() - 1],
    );
    buffers.into_mesh()
}

#[derive(Clone, Copy)]
struct RibbonJoin {
    incoming: [Vec2; 2],
    outgoing: [Vec2; 2],
    bevel: Option<[Vec2; 2]>,
}

impl RibbonJoin {
    fn endpoint(point: Vec2, direction: Vec2, radius: f32) -> Self {
        let normal = direction.perp() * radius;
        let sides = [point + normal, point - normal];
        Self {
            incoming: sides,
            outgoing: sides,
            bevel: None,
        }
    }

    fn between(point: Vec2, incoming: Vec2, outgoing: Vec2, radius: f32) -> Self {
        let incoming_normal = incoming.perp();
        let outgoing_normal = outgoing.perp();
        let normal_sum = incoming_normal + outgoing_normal;

        // A reversal has no finite offset-line intersection. Keeping separate
        // butt ends lets both segment quads remain well formed instead of
        // swapping a shared left/right pair and creating bow-tie triangles.
        if normal_sum.length_squared() <= POINT_EPSILON_SQUARED {
            return Self {
                incoming: [
                    point + incoming_normal * radius,
                    point - incoming_normal * radius,
                ],
                outgoing: [
                    point + outgoing_normal * radius,
                    point - outgoing_normal * radius,
                ],
                bevel: None,
            };
        }

        let miter = normal_sum.normalize();
        let miter_length = radius / miter.dot(outgoing_normal).abs().max(0.001);
        if miter_length <= radius * MITER_LIMIT {
            let offset = miter * miter_length;
            let sides = [point + offset, point - offset];
            return Self {
                incoming: sides,
                outgoing: sides,
                bevel: None,
            };
        }

        let miter_offset = miter * radius * MITER_LIMIT;
        let turn_left = incoming.perp_dot(outgoing) > 0.0;
        if turn_left {
            let incoming_outer = point - incoming_normal * radius;
            let outgoing_outer = point - outgoing_normal * radius;
            Self {
                incoming: [point + miter_offset, incoming_outer],
                outgoing: [point + miter_offset, outgoing_outer],
                bevel: Some([outgoing_outer, incoming_outer]),
            }
        } else {
            let incoming_outer = point + incoming_normal * radius;
            let outgoing_outer = point + outgoing_normal * radius;
            Self {
                incoming: [incoming_outer, point - miter_offset],
                outgoing: [outgoing_outer, point - miter_offset],
                bevel: Some([incoming_outer, outgoing_outer]),
            }
        }
    }
}

struct RibbonBuffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl RibbonBuffers {
    fn with_capacity(point_count: usize) -> Self {
        Self {
            positions: Vec::with_capacity(point_count * 2 + 16),
            normals: Vec::with_capacity(point_count * 2 + 16),
            uvs: Vec::with_capacity(point_count * 2 + 16),
            indices: Vec::with_capacity(point_count.saturating_sub(1) * 6 + 24),
        }
    }

    fn push_vertex(&mut self, point: Vec2, distance: f32, across: f32) {
        self.positions
            .push([point.x, TRAIL_TRAVERSAL_HEIGHT, point.y]);
        self.normals.push([0.0, 1.0, 0.0]);
        self.uvs.push([distance, across]);
    }

    fn triangle(&mut self, a: Vec2, b: Vec2, c: Vec2, distance: f32) {
        let base = self.positions.len() as u32;
        for point in [a, b, c] {
            self.push_vertex(point, distance, 0.5);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    fn into_mesh(self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

fn append_round_cap(
    buffers: &mut RibbonBuffers,
    previous: Vec2,
    center: Vec2,
    radius: f32,
    along: f32,
) {
    let direction = (center - previous).normalize_or(Vec2::X);
    let left = Vec2::new(-direction.y, direction.x);
    // The arc runs from one side of the ribbon to the other through the
    // trailing direction. It is intentionally a semicircle, not a disk.
    let start = left.to_angle();
    let end = start + std::f32::consts::PI;
    let base = buffers.positions.len() as u32;
    buffers.push_vertex(center, along, 0.5);
    for segment in 0..=CAP_SEGMENTS {
        let angle = start + (end - start) * segment as f32 / CAP_SEGMENTS as f32;
        let point = center + Vec2::from_angle(angle) * radius;
        buffers.push_vertex(point, along, 0.5 + angle.sin() * 0.5);
    }
    for segment in 0..CAP_SEGMENTS {
        let current = base + 1 + segment as u32;
        let next = current + 1;
        // Reverse perimeter order so the top face keeps an upward normal.
        buffers.indices.extend_from_slice(&[base, next, current]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ribbon_has_one_quad_per_segment() {
        let mesh = ribbon_mesh(&[Vec2::ZERO, Vec2::X, Vec2::new(1.0, 1.0)], 0.65);
        assert_eq!(mesh.count_vertices(), 8 + CAP_SEGMENTS + 2);
        assert_eq!(mesh.indices().unwrap().len(), 12 + CAP_SEGMENTS * 3);
    }

    #[test]
    fn anchored_end_does_not_bulge_behind_the_boundary() {
        let mesh = ribbon_mesh(&[Vec2::ZERO, Vec2::X], 0.65);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert!(positions.iter().all(|position| position[0] >= -0.001));
        // The cap is behind the head, so it must not extend in the direction
        // of travel beyond the head position.
        assert!(positions.iter().all(|position| position[0] <= 1.001));
    }

    #[test]
    fn acute_join_stays_within_the_miter_limit() {
        let width = 0.65;
        let join = Vec2::X;
        let mesh = ribbon_mesh(&[Vec2::ZERO, join, Vec2::new(0.1, 0.01)], width);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let maximum_join_offset = width * 0.5 * MITER_LIMIT + 0.001;
        assert!(positions[2..6].iter().all(|position| {
            Vec2::new(position[0], position[2]).distance(join) <= maximum_join_offset
        }));
    }

    #[test]
    fn exact_reversal_produces_two_valid_quads() {
        let mesh = ribbon_mesh(&[Vec2::ZERO, Vec2::X, Vec2::ZERO], 0.65);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let indices = mesh.indices().unwrap();

        assert_eq!(indices.iter().take(12).count(), 12);
        for triangle in indices.iter().take(12).collect::<Vec<_>>().chunks_exact(3) {
            let [a, b, c] = [
                positions[triangle[0]],
                positions[triangle[1]],
                positions[triangle[2]],
            ];
            let ab = Vec2::new(b[0] - a[0], b[2] - a[2]);
            let ac = Vec2::new(c[0] - a[0], c[2] - a[2]);
            assert!(ab.perp_dot(ac) < -f32::EPSILON);
        }
    }

    #[test]
    fn ribbon_stays_flat_on_the_traversal_plane() {
        let mesh = ribbon_mesh(&[Vec2::new(0.5, 0.5), Vec2::new(1.5, 0.5)], 0.65);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert!(
            positions
                .iter()
                .all(|position| position[1] == TRAIL_TRAVERSAL_HEIGHT)
        );
    }

    #[test]
    fn unsampled_trail_head_is_renderable_from_the_boundary() {
        let mut active = crate::trail::ActiveTrail::new(
            crate::ids::CompetitorId(0),
            crate::board::Cell::new(0, 0),
            Vec2::ZERO,
            Vec2::X,
        );
        active.head = Vec2::new(0.1, 0.0);
        let visual = TrailVisual {
            points: active.render_points(MAX_RENDER_TRAIL_POINTS),
            source_samples: active.points.len(),
            ..default()
        };

        assert_eq!(visual.source_samples, 1);
        assert_eq!(visual.points, vec![Vec2::ZERO, Vec2::new(0.1, 0.0)]);
        assert!(trail_has_ribbon_geometry(&visual));
    }
}
