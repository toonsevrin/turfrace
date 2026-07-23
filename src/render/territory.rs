use std::collections::{HashMap, HashSet};

use bevy::{
    asset::RenderAssetUsages, ecs::system::SystemParam, mesh::Indices, prelude::*,
    render::render_resource::PrimitiveTopology,
};

use crate::{geometry::MultiPolygon, palette::palette_color};

use super::{PresentationSettings, RetiredMeshes, materials::TerritoryMaterial, mix_with_white};

pub const FIELD_SURFACE_HEIGHT: f32 = 0.015;
pub const TERRITORY_SURFACE_HEIGHT: f32 = 0.17;
const TERRITORY_WALL_BASE: f32 = 0.025;

/// Compact render snapshot of the authoritative board. The simulation grid is
/// never shown directly: ownership is rebuilt into a small number of exact
/// owner surfaces only when this revision changes.
#[derive(Resource, Debug, Clone)]
pub struct TerritoryVisual {
    pub width: u32,
    pub height: u32,
    pub cell_size: f32,
    pub origin: Vec2,
    pub owners: Vec<u8>,
    /// Exact vector geometry copied from `TerritoryMap` on revision changes.
    pub polygons: [MultiPolygon; 12],
    pub arena: MultiPolygon,
    pub pattern_ids: [u8; 12],
    pub color_ids: [u8; 12],
    pub revision: u64,
}

impl Default for TerritoryVisual {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            cell_size: 0.5,
            origin: Vec2::ZERO,
            owners: Vec::new(),
            polygons: std::array::from_fn(|_| MultiPolygon::empty()),
            arena: MultiPolygon::empty(),
            pattern_ids: [0; 12],
            color_ids: std::array::from_fn(|index| index as u8),
            revision: 0,
        }
    }
}

impl TerritoryVisual {
    pub fn is_valid(&self) -> bool {
        (self.width > 0
            && self.height > 0
            && self.cell_size > 0.0
            && self.owners.len() == self.width as usize * self.height as usize)
            || !self.arena.is_empty()
    }

    pub fn owner_at(&self, point: Vec2) -> u8 {
        for (index, polygon) in self.polygons.iter().enumerate() {
            if polygon.contains_world(point) {
                return index as u8 + 1;
            }
        }
        if !self.is_valid() {
            return 0;
        }
        let cell = ((point - self.origin) / self.cell_size).floor().as_ivec2();
        if cell.x < 0 || cell.y < 0 || cell.x >= self.width as i32 || cell.y >= self.height as i32 {
            return 0;
        }
        self.owners[cell.y as usize * self.width as usize + cell.x as usize]
    }

    pub fn surface_height(&self, point: Vec2) -> f32 {
        if self.owner_at(point) == 0 {
            FIELD_SURFACE_HEIGHT
        } else {
            TERRITORY_SURFACE_HEIGHT
        }
    }

    pub fn arena_contains(&self, point: Vec2) -> bool {
        if !self.arena.is_empty() {
            self.arena.contains_world(point)
        } else {
            self.is_valid()
        }
    }
}

#[derive(Component)]
pub(super) struct TerritorySurface {
    owner: u8,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct GridPoint {
    x: i32,
    y: i32,
}

#[derive(SystemParam)]
pub(super) struct TerritoryAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<TerritoryMaterial>>,
    retired: ResMut<'w, RetiredMeshes>,
}

type ExistingSurface = (Entity, Handle<Mesh>, Handle<TerritoryMaterial>);

/// Rebuilds at most one exact mesh per owner on ownership changes. The board
/// has no entity per cell, and ordinary frames do no territory work.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_territory_surface(
    mut commands: Commands,
    territory: Res<TerritoryVisual>,
    settings: Res<PresentationSettings>,
    mut assets: TerritoryAssets,
    surface: Query<(
        Entity,
        &TerritorySurface,
        &Mesh3d,
        &MeshMaterial3d<TerritoryMaterial>,
    )>,
    mut last_revision: Local<u64>,
) {
    if !territory.is_valid() {
        for (entity, _, _, _) in &surface {
            commands.entity(entity).despawn();
        }
        *last_revision = territory.revision;
        return;
    }
    if *last_revision == territory.revision && !settings.is_changed() {
        return;
    }

    let mut existing: [Option<ExistingSurface>; 12] = std::array::from_fn(|_| None);
    for (entity, surface, mesh, material) in &surface {
        if (1..=12).contains(&surface.owner) {
            existing[surface.owner as usize - 1] =
                Some((entity, mesh.0.clone(), material.0.clone()));
        } else {
            commands.entity(entity).despawn();
        }
    }

    for owner in 1..=12_u8 {
        let mesh = build_owner_mesh(&territory, owner);
        let Some(mesh) = mesh else {
            if let Some((entity, _, _)) = existing[owner as usize - 1].take() {
                commands.entity(entity).despawn();
            }
            continue;
        };
        let material_value = territory_material(&territory, owner, settings.territory_patterns);
        if let Some((entity, old_mesh, material_handle)) = existing[owner as usize - 1].take() {
            if let Some(mut material) = assets.materials.get_mut(&material_handle) {
                *material = material_value;
            }
            assets.retired.0.push_back((old_mesh, 0));
            commands.entity(entity).insert((
                Mesh3d(assets.meshes.add(mesh)),
                MeshMaterial3d(material_handle),
            ));
        } else {
            let material_handle = assets.materials.add(material_value);
            commands.spawn((
                Name::new(format!("Territory Owner {owner}")),
                TerritorySurface { owner },
                Mesh3d(assets.meshes.add(mesh)),
                MeshMaterial3d(material_handle),
            ));
        }
    }
    *last_revision = territory.revision;
}

fn territory_material(
    territory: &TerritoryVisual,
    owner: u8,
    patterns_enabled: bool,
) -> TerritoryMaterial {
    let slot = owner as usize - 1;
    TerritoryMaterial {
        color: LinearRgba::from(mix_with_white(
            palette_color(territory.color_ids[slot]),
            0.12,
        )),
        parameters: Vec4::new(
            f32::from(territory.pattern_ids[slot] % 12),
            if patterns_enabled { 1.0 } else { 0.0 },
            0.0,
            0.0,
        ),
    }
}

fn build_owner_mesh(territory: &TerritoryVisual, owner: u8) -> Option<Mesh> {
    let vector_geometry = &territory.polygons[owner as usize - 1];
    if vector_geometry.is_empty() && boundary_loops(territory, owner).is_empty() {
        return None;
    }

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut wall_uvs = Vec::new();
    let mut indices = Vec::new();
    let surface_height = TERRITORY_SURFACE_HEIGHT;
    let wall_base = TERRITORY_WALL_BASE;

    let vector_polygons: Vec<_> = vector_geometry
        .polygons
        .iter()
        .map(|polygon| {
            (
                polygon
                    .outer
                    .iter()
                    .map(|point| point.world())
                    .collect::<Vec<_>>(),
                polygon
                    .holes
                    .iter()
                    .map(|hole| hole.iter().map(|point| point.world()).collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let polygons = if vector_polygons.is_empty() {
        let loops = boundary_loops(territory, owner);
        let mut outers = Vec::new();
        let mut holes = Vec::new();
        for loop_points in loops {
            if polygon_area(&loop_points) >= 0.0 {
                outers.push(loop_points);
            } else {
                holes.push(loop_points);
            }
        }
        outers
            .into_iter()
            .map(|outer| {
                let matching = holes
                    .iter()
                    .filter(|hole| {
                        hole.first()
                            .is_some_and(|point| point_in_polygon(*point, &outer))
                    })
                    .cloned()
                    .collect();
                (outer, matching)
            })
            .collect()
    } else {
        vector_polygons
    };

    for (outer, holes) in polygons {
        let mut coordinates = Vec::new();
        append_coordinates(&mut coordinates, &outer);
        let mut hole_indices = Vec::new();
        for hole in &holes {
            hole_indices.push(coordinates.len() / 2);
            append_coordinates(&mut coordinates, hole);
        }
        let Ok(triangles) = earcutr::earcut(&coordinates, &hole_indices, 2) else {
            continue;
        };
        let base = positions.len() as u32;
        for pair in coordinates.chunks_exact(2) {
            let point = Vec2::new(pair[0] as f32, pair[1] as f32);
            positions.push([point.x, surface_height, point.y]);
            normals.push([0.0, 1.0, 0.0]);
            uvs.push([point.x, point.y]);
            wall_uvs.push([0.0, 0.0]);
        }
        indices.extend(triangles.into_iter().map(|index| base + index as u32));
    }

    let wall_loops: Vec<(Vec<Vec2>, bool)> = if vector_geometry.is_empty() {
        boundary_loops(territory, owner)
            .into_iter()
            .map(|points| (points.clone(), polygon_area(&points) < 0.0))
            .collect()
    } else {
        vector_geometry
            .polygons
            .iter()
            .flat_map(|polygon| {
                std::iter::once((
                    polygon.outer.iter().map(|point| point.world()).collect(),
                    false,
                ))
                .chain(
                    polygon
                        .holes
                        .iter()
                        .map(|hole| (hole.iter().map(|point| point.world()).collect(), true)),
                )
            })
            .collect()
    };
    for (loop_points, is_hole) in wall_loops {
        let base = positions.len() as u32;
        for (index, point) in loop_points.iter().copied().enumerate() {
            let next = loop_points[(index + 1) % loop_points.len()];
            let direction = (next - point).normalize_or_zero();
            let outward = if is_hole {
                Vec2::new(-direction.y, direction.x)
            } else {
                Vec2::new(direction.y, -direction.x)
            };
            positions.push([point.x, surface_height, point.y]);
            positions.push([point.x, wall_base, point.y]);
            normals.push([outward.x, 0.0, outward.y]);
            normals.push([outward.x, 0.0, outward.y]);
            uvs.extend_from_slice(&[[point.x, point.y], [point.x, point.y]]);
            wall_uvs.extend_from_slice(&[[1.0, 1.0], [1.0, 1.0]]);
        }
        for index in 0..loop_points.len() {
            let next = (index + 1) % loop_points.len();
            let a = base + (index * 2) as u32;
            let b = a + 1;
            let c = base + (next * 2) as u32;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    if indices.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, wall_uvs);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

fn boundary_loops(territory: &TerritoryVisual, owner: u8) -> Vec<Vec<Vec2>> {
    let mut edges = Vec::new();
    for y in 0..territory.height as i32 {
        for x in 0..territory.width as i32 {
            if territory.owners[y as usize * territory.width as usize + x as usize] != owner {
                continue;
            }
            let cell = [
                GridPoint { x, y },
                GridPoint { x: x + 1, y },
                GridPoint { x: x + 1, y: y + 1 },
                GridPoint { x, y: y + 1 },
            ];
            let neighbors = [
                (x, y - 1, cell[0], cell[1]),
                (x + 1, y, cell[1], cell[2]),
                (x, y + 1, cell[2], cell[3]),
                (x - 1, y, cell[3], cell[0]),
            ];
            for (nx, ny, start, end) in neighbors {
                let same = nx >= 0
                    && ny >= 0
                    && nx < territory.width as i32
                    && ny < territory.height as i32
                    && territory.owners[ny as usize * territory.width as usize + nx as usize]
                        == owner;
                if !same {
                    edges.push((start, end));
                }
            }
        }
    }
    let mut outgoing: HashMap<GridPoint, Vec<GridPoint>> = HashMap::new();
    for (start, end) in &edges {
        outgoing.entry(*start).or_default().push(*end);
    }
    let mut unused: HashSet<(GridPoint, GridPoint)> = edges.into_iter().collect();
    let mut loops = Vec::new();
    while let Some(&(start, _)) = unused.iter().min_by_key(|(a, b)| (a.y, a.x, b.y, b.x)) {
        let mut points = Vec::new();
        let mut current = start;
        loop {
            points.push(current);
            let Some(candidates) = outgoing.get(&current) else {
                break;
            };
            let next = candidates
                .iter()
                .copied()
                .filter(|candidate| unused.contains(&(current, *candidate)))
                .min_by_key(|candidate| (candidate.y, candidate.x));
            let Some(next) = next else { break };
            unused.remove(&(current, next));
            current = next;
            if current == start || points.len() > territory.owners.len() * 4 {
                break;
            }
        }
        if points.len() >= 3 {
            let world: Vec<_> = points
                .into_iter()
                .map(|point| {
                    territory.origin
                        + Vec2::new(point.x as f32, point.y as f32) * territory.cell_size
                })
                .collect();
            if polygon_area(&world).abs() > territory.cell_size * territory.cell_size * 0.25 {
                loops.push(world);
            }
        }
    }
    loops
}

fn append_coordinates(coordinates: &mut Vec<f64>, points: &[Vec2]) {
    coordinates.extend(
        points
            .iter()
            .flat_map(|point| [f64::from(point.x), f64::from(point.y)]),
    );
}

fn polygon_area(points: &[Vec2]) -> f32 {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum::<f32>()
        * 0.5
}

fn point_in_polygon(point: Vec2, polygon: &[Vec2]) -> bool {
    polygon
        .iter()
        .copied()
        .zip(polygon.iter().copied().cycle().skip(1))
        .fold(false, |inside, (a, b)| {
            if (a.y > point.y) != (b.y > point.y)
                && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
            {
                !inside
            } else {
                inside
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn territory() -> TerritoryVisual {
        TerritoryVisual {
            width: 5,
            height: 5,
            cell_size: 1.0,
            origin: Vec2::ZERO,
            owners: vec![
                0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0,
            ],
            ..default()
        }
    }

    #[test]
    fn owner_mesh_has_exact_top_and_elevated_wall() {
        let mesh = build_owner_mesh(&territory(), 1).expect("owner mesh");
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert!(
            positions
                .iter()
                .any(|position| position[1] >= TERRITORY_SURFACE_HEIGHT)
        );
        assert!(
            positions
                .iter()
                .any(|position| position[1] >= TERRITORY_WALL_BASE)
        );
        assert!(mesh.count_vertices() > 12);
    }

    #[test]
    fn boundary_loops_round_trip_to_world_coordinates() {
        let loops = boundary_loops(&territory(), 1);
        assert_eq!(loops.len(), 1);
        assert!(
            loops[0]
                .iter()
                .all(|point| point.x >= 1.0 && point.x <= 4.0)
        );
    }

    #[test]
    fn surface_height_tracks_claimed_cells_and_bounds() {
        let board = territory();
        assert_eq!(
            board.surface_height(Vec2::splat(1.5)),
            TERRITORY_SURFACE_HEIGHT
        );
        assert_eq!(board.surface_height(Vec2::ZERO), FIELD_SURFACE_HEIGHT);
        assert_eq!(
            board.surface_height(Vec2::splat(20.0)),
            FIELD_SURFACE_HEIGHT
        );
    }

    #[test]
    fn vector_surface_keeps_exact_edges_and_holes() {
        let mut visual = TerritoryVisual::default();
        visual.polygons[0] = crate::geometry::MultiPolygon::from_outer(&[
            Vec2::new(-4.0, -4.0),
            Vec2::new(4.0, -4.0),
            Vec2::new(4.0, 4.0),
            Vec2::new(-4.0, 4.0),
        ])
        .difference(&crate::geometry::MultiPolygon::from_outer(&[
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ]));
        let mesh = build_owner_mesh(&visual, 1).expect("vector owner mesh");
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert!(positions.iter().any(|point| {
            (point[0].abs() - 4.0).abs() < 0.001 && (point[2].abs() - 4.0).abs() < 0.001
        }));
        assert!(visual.polygons[0].contains_world(Vec2::new(3.0, 0.0)));
        assert!(!visual.polygons[0].contains_world(Vec2::ZERO));
    }
}
