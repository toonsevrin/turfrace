use std::collections::{HashMap, HashSet};

use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};

use super::{
    GraphicsQuality, PresentationSettings, RetiredMeshes,
    materials::{RenderAssets, TerritoryMaterial},
    mix_with_white, palette_color,
};

pub const CHUNK_SIZE: u32 = 32;

// Ownership is authoritative on the half-unit grid, but its presentation should
// read as one continuous painted surface.  These values are deliberately small
// relative to a cell: they soften the stepped silhouette without changing the
// playable or collision geometry.
const EDGE_RISE: f32 = 0.112;

/// Dense ownership snapshot optimized for chunk rebuilding, not gameplay queries.
#[derive(Resource, Debug, Clone, Default)]
pub struct TerritoryVisual {
    pub width: u32,
    pub height: u32,
    pub cell_size: f32,
    pub origin: Vec2,
    /// Zero is unclaimed; 1..=12 maps to the visual palette.
    pub owners: Vec<u8>,
    pub playable: Vec<bool>,
    /// Pattern selected by each stable owner ID.
    pub pattern_ids: [u8; 12],
    /// Color selected by each stable owner ID.
    pub color_ids: [u8; 12],
    pub revision: u64,
    /// If empty after a revision, every chunk is considered dirty.
    pub dirty_chunks: Vec<UVec2>,
}

impl TerritoryVisual {
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.cell_size > 0.0
            && self.owners.len() == (self.width * self.height) as usize
            && self.playable.len() == self.owners.len()
    }

    fn owner(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return 0;
        }
        self.owners[(y as u32 * self.width + x as u32) as usize]
    }
}

#[derive(Component)]
pub(super) struct TerritoryChunk(UVec2);

#[derive(Component)]
pub(super) struct TerritoryChunkAnimation(f32);

pub(super) fn sync_territory_chunks(
    mut commands: Commands,
    territory: Res<TerritoryVisual>,
    render_assets: Option<Res<RenderAssets>>,
    mut retired: ResMut<RetiredMeshes>,
    mut meshes: ResMut<Assets<Mesh>>,
    chunks: Query<(Entity, &TerritoryChunk, &Mesh3d)>,
    mut last_revision: Local<Option<u64>>,
) {
    if last_revision.as_ref() == Some(&territory.revision) || !territory.is_valid() {
        return;
    }
    let Some(render_assets) = render_assets else {
        return;
    };
    *last_revision = Some(territory.revision);
    let existing: HashMap<UVec2, (Entity, Handle<Mesh>)> = chunks
        .iter()
        .map(|(entity, chunk, mesh)| (chunk.0, (entity, mesh.0.clone())))
        .collect();
    let chunk_columns = territory.width.div_ceil(CHUNK_SIZE);
    let chunk_rows = territory.height.div_ceil(CHUNK_SIZE);
    for (entity, chunk, _) in &chunks {
        if chunk.0.x >= chunk_columns || chunk.0.y >= chunk_rows {
            commands.entity(entity).despawn();
        }
    }
    let dirty: HashSet<UVec2> = if territory.dirty_chunks.is_empty() {
        (0..chunk_rows)
            .flat_map(|y| (0..chunk_columns).map(move |x| UVec2::new(x, y)))
            .collect()
    } else {
        territory.dirty_chunks.iter().copied().collect()
    };

    for chunk in dirty {
        if chunk.x >= chunk_columns || chunk.y >= chunk_rows {
            continue;
        }
        let rebuilt = build_chunk_mesh(&territory, chunk);
        if rebuilt.count_vertices() == 0 {
            if let Some((entity, old_handle)) = existing.get(&chunk) {
                retired.0.push_back((old_handle.clone(), 0));
                commands.entity(*entity).despawn();
            }
            continue;
        }
        if let Some((entity, old_handle)) = existing.get(&chunk) {
            // Replace the component handle rather than mutating an already
            // extracted Mesh asset in place.  WebGL2 drivers can still be
            // reading the old slab allocation when the CPU rebuilds a chunk;
            // a fresh handle lets Bevy retire that allocation safely.
            let replacement = meshes.add(rebuilt);
            retired.0.push_back((old_handle.clone(), 0));
            commands.entity(*entity).insert(Mesh3d(replacement));
        } else {
            commands.spawn((
                Name::new(format!("Territory Chunk {},{}", chunk.x, chunk.y)),
                TerritoryChunk(chunk),
                TerritoryChunkAnimation(0.25),
                Mesh3d(meshes.add(rebuilt)),
                MeshMaterial3d::<TerritoryMaterial>(render_assets.territory_material.clone()),
            ));
        }
    }
}

pub(super) fn animate_territory_chunks(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<PresentationSettings>,
    mut chunks: Query<(Entity, &mut TerritoryChunkAnimation, &mut Transform)>,
) {
    for (entity, mut animation, mut transform) in &mut chunks {
        if settings.reduced_motion || settings.quality == GraphicsQuality::Low {
            transform.translation.y = 0.0;
            commands.entity(entity).remove::<TerritoryChunkAnimation>();
            continue;
        }
        animation.0 = (animation.0 - time.delta_secs()).max(0.0);
        let remaining = animation.0 / 0.25;
        transform.translation.y = -0.10 * remaining * remaining;
        if animation.0 <= 0.0 {
            transform.translation.y = 0.0;
            commands.entity(entity).remove::<TerritoryChunkAnimation>();
        }
    }
}

fn build_chunk_mesh(board: &TerritoryVisual, chunk: UVec2) -> Mesh {
    let start_x = chunk.x * CHUNK_SIZE;
    let start_y = chunk.y * CHUNK_SIZE;
    let end_x = (start_x + CHUNK_SIZE).min(board.width);
    let end_y = (start_y + CHUNK_SIZE).min(board.height);
    let mut builder = MeshBuilder::default();
    let chunk_min = board.origin + Vec2::new(start_x as f32, start_y as f32) * board.cell_size;
    let chunk_max = board.origin + Vec2::new(end_x as f32, end_y as f32) * board.cell_size;
    let mut owners = HashSet::new();
    for y in start_y.saturating_sub(1)..=(end_y.min(board.height)) {
        for x in start_x.saturating_sub(1)..=(end_x.min(board.width)) {
            let owner = board.owner(x as i32, y as i32);
            if owner != 0 {
                owners.insert(owner);
            }
        }
    }
    for owner in owners {
        let identity = (owner - 1) as usize;
        let color = mix_with_white(palette_color(board.color_ids[identity]), 0.20)
            .to_linear()
            .to_f32_array();
        let pattern = f32::from(board.pattern_ids[identity]);
        for y in (start_y as i32 - 1)..(end_y as i32) {
            for x in (start_x as i32 - 1)..(end_x as i32) {
                let pieces = marching_square(board, owner, x, y);
                for piece in pieces {
                    let clipped = clip_convex_polygon(&piece, chunk_min, chunk_max);
                    if clipped.len() >= 3 {
                        builder.convex_polygon(&clipped, color, pattern, EDGE_RISE);
                    }
                }
                for (a, b) in marching_square_boundary(board, owner, x, y) {
                    if segment_in_rect(a, b, chunk_min, chunk_max) {
                        builder.boundary_wall(a, b, darken(color, 0.18), pattern, EDGE_RISE);
                    }
                }
            }
        }
    }
    builder.finish()
}

fn darken(mut color: [f32; 4], amount: f32) -> [f32; 4] {
    color[0] *= 1.0 - amount;
    color[1] *= 1.0 - amount;
    color[2] *= 1.0 - amount;
    color
}

/// Samples a 2×2 dual-grid square around cell centers. Every case is emitted
/// as convex pieces; diagonal cases deliberately split into two wedges. This
/// avoids self-intersections in concave captures while preserving a shared
/// edge between neighboring squares.
fn marching_square(board: &TerritoryVisual, owner: u8, x: i32, y: i32) -> Vec<Vec<Vec2>> {
    let corners = [
        dual_point(board, x, y),
        dual_point(board, x + 1, y),
        dual_point(board, x + 1, y + 1),
        dual_point(board, x, y + 1),
    ];
    let occupied = [
        board.owner(x, y) == owner,
        board.owner(x + 1, y) == owner,
        board.owner(x + 1, y + 1) == owner,
        board.owner(x, y + 1) == owner,
    ];
    marching_square_values(
        corners,
        occupied.map(|occupied| if occupied { 1.0 } else { 0.0 }),
    )
}

fn marching_square_values(corners: [Vec2; 4], values: [f32; 4]) -> Vec<Vec<Vec2>> {
    let occupied = values.map(|value| value >= 0.5);
    let mask = occupied
        .iter()
        .enumerate()
        .fold(0u8, |mask, (index, value)| mask | ((*value as u8) << index));
    if mask == 0 {
        return Vec::new();
    }
    let midpoint = [
        edge_crossing(corners[0], corners[1], values[0], values[1]),
        edge_crossing(corners[1], corners[2], values[1], values[2]),
        edge_crossing(corners[2], corners[3], values[2], values[3]),
        edge_crossing(corners[3], corners[0], values[3], values[0]),
    ];
    if mask == 5 {
        return vec![
            vec![corners[0], midpoint[0], midpoint[3]],
            vec![corners[2], midpoint[2], midpoint[1]],
        ];
    }
    if mask == 10 {
        return vec![
            vec![corners[1], midpoint[1], midpoint[0]],
            vec![corners[3], midpoint[3], midpoint[2]],
        ];
    }
    let mut polygon = Vec::with_capacity(8);
    for index in 0..4 {
        if occupied[index] {
            polygon.push(corners[index]);
        }
        if occupied[index] != occupied[(index + 1) % 4] {
            polygon.push(midpoint[index]);
        }
    }
    vec![polygon]
}

fn edge_crossing(a: Vec2, b: Vec2, a_value: f32, b_value: f32) -> Vec2 {
    let denominator = b_value - a_value;
    let amount = if denominator.abs() < 0.000_001 {
        0.5
    } else {
        ((0.5 - a_value) / denominator).clamp(0.0, 1.0)
    };
    a.lerp(b, amount)
}

fn dual_point(board: &TerritoryVisual, x: i32, y: i32) -> Vec2 {
    board.origin + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * board.cell_size
}

fn marching_square_boundary(
    board: &TerritoryVisual,
    owner: u8,
    x: i32,
    y: i32,
) -> Vec<(Vec2, Vec2)> {
    let corners = [
        dual_point(board, x, y),
        dual_point(board, x + 1, y),
        dual_point(board, x + 1, y + 1),
        dual_point(board, x, y + 1),
    ];
    let occupied = [
        board.owner(x, y) == owner,
        board.owner(x + 1, y) == owner,
        board.owner(x + 1, y + 1) == owner,
        board.owner(x, y + 1) == owner,
    ];
    let midpoint = [
        corners[0].lerp(corners[1], 0.5),
        corners[1].lerp(corners[2], 0.5),
        corners[2].lerp(corners[3], 0.5),
        corners[3].lerp(corners[0], 0.5),
    ];
    let transitions: Vec<_> = (0..4)
        .filter(|edge| occupied[*edge] != occupied[(*edge + 1) % 4])
        .collect();
    match transitions.as_slice() {
        [a, b] => vec![(midpoint[*a], midpoint[*b])],
        [a, b, c, d] => {
            let mask = occupied
                .iter()
                .enumerate()
                .fold(0u8, |mask, (index, value)| mask | ((*value as u8) << index));
            if mask == 5 {
                vec![(midpoint[*a], midpoint[*d]), (midpoint[*b], midpoint[*c])]
            } else {
                vec![(midpoint[*a], midpoint[*b]), (midpoint[*c], midpoint[*d])]
            }
        }
        _ => Vec::new(),
    }
}

fn clip_convex_polygon(points: &[Vec2], min: Vec2, max: Vec2) -> Vec<Vec2> {
    let mut clipped = points.to_vec();
    for (axis, limit, keep_greater) in [
        (0, min.x, true),
        (0, max.x, false),
        (1, min.y, true),
        (1, max.y, false),
    ] {
        let previous_points = std::mem::take(&mut clipped);
        if previous_points.is_empty() {
            break;
        }
        let inside = |point: Vec2| {
            let value = if axis == 0 { point.x } else { point.y };
            if keep_greater {
                value >= limit
            } else {
                value <= limit
            }
        };
        let coordinate = |point: Vec2| if axis == 0 { point.x } else { point.y };
        let mut previous = *previous_points.last().unwrap();
        for current in previous_points {
            let previous_inside = inside(previous);
            let current_inside = inside(current);
            if previous_inside != current_inside {
                let denominator = coordinate(current) - coordinate(previous);
                let amount = if denominator.abs() < 0.000_001 {
                    0.0
                } else {
                    (limit - coordinate(previous)) / denominator
                };
                clipped.push(previous.lerp(current, amount.clamp(0.0, 1.0)));
            }
            if current_inside {
                clipped.push(current);
            }
            previous = current;
        }
    }
    clipped
}

fn segment_in_rect(a: Vec2, b: Vec2, min: Vec2, max: Vec2) -> bool {
    let on_chunk_cut = (a.x - b.x).abs() < 0.000_001
        && ((a.x - min.x).abs() < 0.000_001 || (a.x - max.x).abs() < 0.000_001)
        || (a.y - b.y).abs() < 0.000_001
            && ((a.y - min.y).abs() < 0.000_001 || (a.y - max.y).abs() < 0.000_001);
    if on_chunk_cut {
        return false;
    }
    [a, b]
        .into_iter()
        .all(|point| point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y)
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GridEdge {
    start: IVec2,
    end: IVec2,
}

/// Extract closed, directed boundary loops for one owner across the whole
/// board. Individual chunk meshes clip these shared loops to their rectangle,
/// so a large capture remains continuous at chunk boundaries.
#[cfg(test)]
fn owner_contours(board: &TerritoryVisual, owner: u8) -> Vec<Vec<Vec2>> {
    let start_x = 0;
    let start_y = 0;
    let end_x = board.width;
    let end_y = board.height;
    let mut edges = Vec::new();
    for y in start_y..end_y {
        for x in start_x..end_x {
            if board.owner(x as i32, y as i32) != owner {
                continue;
            }
            let x = x as i32;
            let y = y as i32;
            let min = IVec2::new(x, y);
            let max = IVec2::new(x + 1, y + 1);
            if board.owner(x - 1, y) != owner {
                edges.push(GridEdge {
                    start: IVec2::new(min.x, max.y),
                    end: min,
                });
            }
            if board.owner(x + 1, y) != owner {
                edges.push(GridEdge {
                    start: IVec2::new(max.x, min.y),
                    end: max,
                });
            }
            if board.owner(x, y - 1) != owner {
                edges.push(GridEdge {
                    start: min,
                    end: IVec2::new(max.x, min.y),
                });
            }
            if board.owner(x, y + 1) != owner {
                edges.push(GridEdge {
                    start: max,
                    end: IVec2::new(min.x, max.y),
                });
            }
        }
    }

    let mut used = vec![false; edges.len()];
    let mut contours = Vec::new();
    for first_index in 0..edges.len() {
        if used[first_index] {
            continue;
        }
        let first = edges[first_index];
        used[first_index] = true;
        let mut path = vec![first.start, first.end];
        let mut previous_direction = first.end - first.start;
        let mut closed = false;
        while let Some(&current) = path.last() {
            if current == first.start {
                closed = true;
                break;
            }
            let mut candidates = edges
                .iter()
                .enumerate()
                .filter(|(index, edge)| !used[*index] && edge.start == current)
                .collect::<Vec<_>>();
            if candidates.is_empty() {
                break;
            }
            candidates.sort_by(|(_, a), (_, b)| {
                let a_direction =
                    Vec2::new((a.end.x - a.start.x) as f32, (a.end.y - a.start.y) as f32);
                let b_direction =
                    Vec2::new((b.end.x - b.start.x) as f32, (b.end.y - b.start.y) as f32);
                let previous = Vec2::new(previous_direction.x as f32, previous_direction.y as f32);
                let a_turn = previous
                    .perp_dot(a_direction)
                    .atan2(previous.dot(a_direction));
                let b_turn = previous
                    .perp_dot(b_direction)
                    .atan2(previous.dot(b_direction));
                b_turn.total_cmp(&a_turn)
            });
            let (next_index, next) = candidates[0];
            used[next_index] = true;
            previous_direction = next.end - next.start;
            path.push(next.end);
            if path.len() > edges.len() + 2 {
                break;
            }
        }
        if closed && path.len() >= 4 {
            path.pop();
            contours.push(
                path.into_iter()
                    .map(|point| {
                        board.origin + Vec2::new(point.x as f32, point.y as f32) * board.cell_size
                    })
                    .collect(),
            );
        }
    }
    contours
}

/// One Chaikin pass rounds the grid corners while keeping the contour inside
/// its cell union at convex corners.  This is intentionally modest: the shape
/// stays legible at high zoom and never gains a large, expensive spline.
#[cfg(test)]
fn smooth_contour(contour: &[Vec2]) -> Vec<Vec2> {
    if contour.len() < 3 {
        return contour.to_vec();
    }
    let mut simplified = Vec::with_capacity(contour.len());
    for &point in contour {
        let previous = simplified.last().copied();
        if previous.is_none_or(|previous| point.distance_squared(previous) > 0.000_001) {
            simplified.push(point);
        }
    }
    let mut changed = true;
    while changed && simplified.len() > 3 {
        changed = false;
        for index in 0..simplified.len() {
            let previous = simplified[(index + simplified.len() - 1) % simplified.len()];
            let point = simplified[index];
            let following = simplified[(index + 1) % simplified.len()];
            if (point - previous).perp_dot(following - point).abs() < 0.000_001 {
                simplified.remove(index);
                changed = true;
                break;
            }
        }
    }
    let mut rounded = Vec::with_capacity(simplified.len() * 2);
    for index in 0..simplified.len() {
        let point = simplified[index];
        let following = simplified[(index + 1) % simplified.len()];
        rounded.push(point.lerp(following, 0.24));
        rounded.push(point.lerp(following, 0.76));
    }
    rounded
}

#[cfg(test)]
fn clip_contour(points: &[Vec2], min: Vec2, max: Vec2) -> Vec<Vec2> {
    let mut clipped = points.to_vec();
    for (axis, limit, keep_greater) in [
        (0, min.x, true),
        (0, max.x, false),
        (1, min.y, true),
        (1, max.y, false),
    ] {
        if clipped.is_empty() {
            break;
        }
        let previous_points = std::mem::take(&mut clipped);
        let mut previous = *previous_points.last().unwrap();
        let previous_inside = |point: Vec2| {
            let value = if axis == 0 { point.x } else { point.y };
            if keep_greater {
                value >= limit
            } else {
                value <= limit
            }
        };
        let coordinate = |point: Vec2| if axis == 0 { point.x } else { point.y };
        for current in previous_points {
            let current_inside = previous_inside(current);
            let was_inside = previous_inside(previous);
            if current_inside != was_inside {
                let denominator = coordinate(current) - coordinate(previous);
                let factor = if denominator.abs() < 0.000_001 {
                    0.0
                } else {
                    (limit - coordinate(previous)) / denominator
                };
                clipped.push(previous.lerp(current, factor.clamp(0.0, 1.0)));
            }
            if current_inside {
                clipped.push(current);
            }
            previous = current;
        }
    }
    clipped
}

#[cfg(test)]
fn polygon_area(points: &[Vec2]) -> f32 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum::<f32>()
        * 0.5
}

#[cfg(test)]
#[allow(dead_code)]
fn point_in_polygon(point: Vec2, polygon: &[Vec2]) -> bool {
    let mut inside = false;
    for (a, b) in polygon
        .iter()
        .copied()
        .zip(polygon.iter().copied().cycle().skip(1))
        .take(polygon.len())
    {
        if (a.y > point.y) != (b.y > point.y)
            && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
        {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
fn point_in_triangle(point: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let ab = (b - a).perp_dot(point - a);
    let bc = (c - b).perp_dot(point - b);
    let ca = (a - c).perp_dot(point - c);
    (ab >= -0.000_001 && bc >= -0.000_001 && ca >= -0.000_001)
        || (ab <= 0.000_001 && bc <= 0.000_001 && ca <= 0.000_001)
}

#[derive(Default)]
struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uv0: Vec<[f32; 2]>,
    uv1: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct MeshCheckpoint {
    positions: usize,
    normals: usize,
    colors: usize,
    uv0: usize,
    uv1: usize,
    indices: usize,
}

impl MeshBuilder {
    #[cfg(test)]
    fn checkpoint(&self) -> MeshCheckpoint {
        MeshCheckpoint {
            positions: self.positions.len(),
            normals: self.normals.len(),
            colors: self.colors.len(),
            uv0: self.uv0.len(),
            uv1: self.uv1.len(),
            indices: self.indices.len(),
        }
    }

    #[cfg(test)]
    fn rollback(&mut self, checkpoint: MeshCheckpoint) {
        self.positions.truncate(checkpoint.positions);
        self.normals.truncate(checkpoint.normals);
        self.colors.truncate(checkpoint.colors);
        self.uv0.truncate(checkpoint.uv0);
        self.uv1.truncate(checkpoint.uv1);
        self.indices.truncate(checkpoint.indices);
    }

    fn quad(&mut self, points: [[f32; 3]; 4], normal: [f32; 3], color: [f32; 4], uv1: [f32; 2]) {
        let base = self.positions.len() as u32;
        self.positions.extend(points);
        self.normals.extend([normal; 4]);
        self.colors.extend([color; 4]);
        self.uv0
            .extend([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        self.uv1.extend([uv1; 4]);
        self.indices
            .extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    }

    fn convex_polygon(&mut self, points: &[Vec2], color: [f32; 4], pattern: f32, height: f32) {
        if points.len() < 3 {
            return;
        }
        let base = self.positions.len() as u32;
        self.positions
            .extend(points.iter().map(|point| [point.x, height, point.y]));
        self.normals
            .extend(std::iter::repeat_n([0.0, 1.0, 0.0], points.len()));
        self.colors.extend(std::iter::repeat_n(color, points.len()));
        self.uv0
            .extend(points.iter().map(|point| [point.x, point.y]));
        self.uv1
            .extend(std::iter::repeat_n([pattern, 0.0], points.len()));
        // Marching-square pieces are convex and ordered counter-clockwise in
        // world X/Z. Reverse each fan triangle for an upward (+Y) normal.
        for index in 1..points.len() - 1 {
            self.indices
                .extend_from_slice(&[base, base + index as u32 + 1, base + index as u32]);
        }
    }

    fn boundary_wall(&mut self, a: Vec2, b: Vec2, color: [f32; 4], pattern: f32, height: f32) {
        let direction = (b - a).normalize_or_zero();
        let outward = Vec2::new(direction.y, -direction.x);
        self.quad(
            [
                [a.x, 0.016, a.y],
                [b.x, 0.016, b.y],
                [b.x, height, b.y],
                [a.x, height, a.y],
            ],
            [outward.x, 0.0, outward.y],
            color,
            [pattern, 1.0],
        );
        self.quad(
            [
                [a.x, 0.016, a.y],
                [a.x, height, a.y],
                [b.x, height, b.y],
                [b.x, 0.016, b.y],
            ],
            [-outward.x, 0.0, -outward.y],
            color,
            [pattern, 1.0],
        );
    }
    #[cfg(test)]
    fn polygon(&mut self, points: &[Vec2], color: [f32; 4], pattern: f32, height: f32) -> bool {
        if points.len() < 3 {
            return false;
        }
        let checkpoint = self.checkpoint();
        let expected_area = polygon_area(points).abs();
        let mut remaining: Vec<usize> = (0..points.len()).collect();
        let ccw = polygon_area(points) >= 0.0;
        let mut guard = 0;
        while remaining.len() > 3 && guard < points.len() * points.len() {
            guard += 1;
            let mut clipped = false;
            for index in 0..remaining.len() {
                let previous = remaining[(index + remaining.len() - 1) % remaining.len()];
                let current = remaining[index];
                let next = remaining[(index + 1) % remaining.len()];
                let turn =
                    (points[current] - points[previous]).perp_dot(points[next] - points[current]);
                if (ccw && turn <= 0.000_001) || (!ccw && turn >= -0.000_001) {
                    continue;
                }
                if remaining.iter().any(|candidate| {
                    *candidate != previous
                        && *candidate != current
                        && *candidate != next
                        && point_in_triangle(
                            points[*candidate],
                            points[previous],
                            points[current],
                            points[next],
                        )
                }) {
                    continue;
                }
                let triangle = if ccw {
                    [previous, next, current]
                } else {
                    [previous, current, next]
                };
                self.triangle(
                    [
                        points[triangle[0]],
                        points[triangle[1]],
                        points[triangle[2]],
                    ],
                    color,
                    pattern,
                    height,
                );
                remaining.remove(index);
                clipped = true;
                break;
            }
            if !clipped {
                self.rollback(checkpoint);
                return false;
            }
        }
        if remaining.len() == 3 {
            let triangle = if ccw {
                [remaining[0], remaining[2], remaining[1]]
            } else {
                [remaining[0], remaining[1], remaining[2]]
            };
            self.triangle(
                [
                    points[triangle[0]],
                    points[triangle[1]],
                    points[triangle[2]],
                ],
                color,
                pattern,
                height,
            );
        }
        let emitted_area = self.triangle_area_sum(checkpoint.indices);
        if expected_area <= 0.000_001 || (emitted_area - expected_area).abs() > expected_area * 0.02
        {
            self.rollback(checkpoint);
            return false;
        }
        true
    }

    #[cfg(test)]
    fn triangle_area_sum(&self, start: usize) -> f32 {
        self.indices[start..]
            .chunks_exact(3)
            .map(|triangle| {
                let a = self.positions[triangle[0] as usize];
                let b = self.positions[triangle[1] as usize];
                let c = self.positions[triangle[2] as usize];
                let a = Vec2::new(a[0], a[2]);
                let b = Vec2::new(b[0], b[2]);
                let c = Vec2::new(c[0], c[2]);
                (b - a).perp_dot(c - a).abs() * 0.5
            })
            .sum()
    }

    #[cfg(test)]
    fn triangle(&mut self, points: [Vec2; 3], color: [f32; 4], pattern: f32, height: f32) {
        let base = self.positions.len() as u32;
        self.positions
            .extend(points.map(|point| [point.x, height, point.y]));
        self.normals.extend([[0.0, 1.0, 0.0]; 3]);
        self.colors.extend([color; 3]);
        self.uv0.extend([[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]]);
        self.uv1.extend([[pattern, 0.0]; 3]);
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    #[cfg(test)]
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    fn contour_ribbon(
        &mut self,
        points: &[Vec2],
        color: [f32; 4],
        pattern: f32,
        height: f32,
        width: f32,
        chunk_min: Vec2,
        chunk_max: Vec2,
    ) {
        for index in 0..points.len() {
            let point = points[index];
            let previous = points[(index + points.len() - 1) % points.len()];
            let following = points[(index + 1) % points.len()];
            let incoming = (point - previous).normalize_or_zero();
            let outgoing = (following - point).normalize_or_zero();
            let normal_a = Vec2::new(-incoming.y, incoming.x);
            let normal_b = Vec2::new(-outgoing.y, outgoing.x);
            let join = (normal_a + normal_b).normalize_or(normal_b);
            let offset = join * (width / join.dot(normal_b).abs().max(0.5));
            let next = (index + 1) % points.len();
            let next_point = points[next];
            let on_chunk_cut = (point.x - next_point.x).abs() < 0.000_001
                && ((point.x - chunk_min.x).abs() < 0.000_001
                    || (point.x - chunk_max.x).abs() < 0.000_001)
                || (point.y - next_point.y).abs() < 0.000_001
                    && ((point.y - chunk_min.y).abs() < 0.000_001
                        || (point.y - chunk_max.y).abs() < 0.000_001);
            if on_chunk_cut {
                continue;
            }
            let next_incoming = (next_point - point).normalize_or_zero();
            let next_outgoing =
                (points[(next + 1) % points.len()] - next_point).normalize_or_zero();
            let next_join = (Vec2::new(-next_incoming.y, next_incoming.x)
                + Vec2::new(-next_outgoing.y, next_outgoing.x))
            .normalize_or_zero();
            let next_offset = next_join
                * (width
                    / next_join
                        .dot(Vec2::new(-next_outgoing.y, next_outgoing.x))
                        .abs()
                        .max(0.5));
            self.quad(
                [
                    [point.x + offset.x, height, point.y + offset.y],
                    [
                        next_point.x + next_offset.x,
                        height,
                        next_point.y + next_offset.y,
                    ],
                    [
                        next_point.x - next_offset.x,
                        height,
                        next_point.y - next_offset.y,
                    ],
                    [point.x - offset.x, height, point.y - offset.y],
                ],
                [0.0, 1.0, 0.0],
                color,
                [pattern, 1.0],
            );
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    fn contour_walls(
        &mut self,
        points: &[Vec2],
        color: [f32; 4],
        pattern: f32,
        height: f32,
        chunk_min: Vec2,
        chunk_max: Vec2,
    ) {
        for index in 0..points.len() {
            let a = points[index];
            let b = points[(index + 1) % points.len()];
            // Clipping a global contour creates a shared edge at chunk
            // boundaries.  Do not add a vertical wall there; the neighboring
            // chunk owns the same edge and will continue the surface.
            let on_chunk_cut = (a.x - b.x).abs() < 0.000_001
                && ((a.x - chunk_min.x).abs() < 0.000_001 || (a.x - chunk_max.x).abs() < 0.000_001)
                || (a.y - b.y).abs() < 0.000_001
                    && ((a.y - chunk_min.y).abs() < 0.000_001
                        || (a.y - chunk_max.y).abs() < 0.000_001);
            if on_chunk_cut {
                continue;
            }
            let tangent = (b - a).normalize_or_zero();
            let outward = Vec2::new(tangent.y, -tangent.x);
            self.quad(
                [
                    [a.x, 0.016, a.y],
                    [b.x, 0.016, b.y],
                    [b.x, height, b.y],
                    [a.x, height, a.y],
                ],
                [outward.x, 0.0, outward.y],
                color,
                [pattern, 1.0],
            );
        }
    }
    #[cfg(test)]
    #[allow(dead_code)]
    fn seam(&mut self, min: Vec2, max: Vec2, side: usize) {
        let width = 0.025;
        let dark = LinearRgba::from(Color::srgb_u8(23, 25, 29)).to_f32_array();
        let points = match side {
            0 | 1 => {
                let x = if side == 0 { min.x } else { max.x };
                [
                    [x - width, 0.103, min.y],
                    [x + width, 0.103, min.y],
                    [x + width, 0.103, max.y],
                    [x - width, 0.103, max.y],
                ]
            }
            _ => {
                let y = if side == 2 { min.y } else { max.y };
                [
                    [min.x, 0.103, y - width],
                    [max.x, 0.103, y - width],
                    [max.x, 0.103, y + width],
                    [min.x, 0.103, y + width],
                ]
            }
        };
        self.quad(points, [0.0, 1.0, 0.0], dark, [0.0, 1.0]);
    }
    fn finish(self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv0);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, self.uv1);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_mismatched_dense_data() {
        let board = TerritoryVisual {
            width: 2,
            height: 2,
            cell_size: 0.5,
            owners: vec![0],
            playable: vec![true; 4],
            ..default()
        };
        assert!(!board.is_valid());
    }
    #[test]
    fn chunk_contains_top_faces_without_one_entity_per_cell() {
        let board = TerritoryVisual {
            width: 2,
            height: 1,
            cell_size: 0.5,
            owners: vec![1, 1],
            playable: vec![true; 2],
            ..default()
        };
        let mesh = build_chunk_mesh(&board, UVec2::ZERO);
        assert!(mesh.count_vertices() >= 8);
        assert_eq!(mesh.primitive_topology(), PrimitiveTopology::TriangleList);
    }

    #[test]
    fn contiguous_owner_forms_a_single_rounded_contour() {
        let board = TerritoryVisual {
            width: 3,
            height: 3,
            cell_size: 1.0,
            owners: vec![1; 9],
            playable: vec![true; 9],
            ..default()
        };
        let contours = owner_contours(&board, 1);
        assert_eq!(contours.len(), 1);
        assert!(contours[0].len() <= 12, "collinear cell edges must merge");
        let rounded = smooth_contour(&contours[0]);
        assert!(rounded.len() >= 4);
        assert!(polygon_area(&rounded).abs() > 1.0);
        let mesh = build_chunk_mesh(&board, UVec2::ZERO);
        assert!(
            mesh.count_vertices() > 20,
            "contour and ribbon must be present"
        );
    }

    #[test]
    fn contour_clipping_keeps_shared_chunk_edges_exact() {
        let polygon = vec![
            Vec2::new(-1.0, -1.0),
            Vec2::new(3.0, -1.0),
            Vec2::new(3.0, 3.0),
            Vec2::new(-1.0, 3.0),
        ];
        let clipped = clip_contour(&polygon, Vec2::ZERO, Vec2::splat(2.0));
        assert!(clipped.iter().all(|point| {
            point.x >= -0.000_001
                && point.x <= 2.000_001
                && point.y >= -0.000_001
                && point.y <= 2.000_001
        }));
        assert!(clipped.iter().any(|point| point.x == 0.0));
        assert!(clipped.iter().any(|point| point.x == 2.0));
    }

    #[test]
    fn contour_fill_triangles_face_up_for_both_windings() {
        for points in [
            vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(2.0, 0.0),
                Vec2::new(2.0, 2.0),
                Vec2::new(0.0, 2.0),
            ],
            vec![
                Vec2::new(0.0, 2.0),
                Vec2::new(2.0, 2.0),
                Vec2::new(2.0, 0.0),
                Vec2::new(0.0, 0.0),
            ],
        ] {
            let mut builder = MeshBuilder::default();
            builder.polygon(&points, [1.0; 4], 0.0, EDGE_RISE);
            let mesh = builder.finish();
            let positions = mesh
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap();
            for triangle in mesh.indices().unwrap().iter().collect::<Vec<_>>().chunks(3) {
                let a = Vec3::from(positions[triangle[0]]);
                let b = Vec3::from(positions[triangle[1]]);
                let c = Vec3::from(positions[triangle[2]]);
                assert!((b - a).cross(c - a).y > 0.0);
            }
        }
    }

    #[test]
    fn chunk_uses_the_competitors_selected_color_and_pattern() {
        let mut board = TerritoryVisual {
            width: 1,
            height: 1,
            cell_size: 0.5,
            owners: vec![1],
            playable: vec![true],
            ..default()
        };
        board.color_ids[0] = 5;
        board.pattern_ids[0] = 9;
        let mesh = build_chunk_mesh(&board, UVec2::ZERO);
        let bevy::mesh::VertexAttributeValues::Float32x4(colors) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap()
        else {
            panic!("territory colors must be float4");
        };
        let bevy::mesh::VertexAttributeValues::Float32x2(patterns) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_1).unwrap()
        else {
            panic!("territory pattern coordinates must be float2");
        };
        let expected = mix_with_white(palette_color(5), 0.20)
            .to_linear()
            .to_f32_array();
        assert_eq!(colors[0], expected);
        assert_eq!(patterns[0][0], 9.0);
    }

    #[test]
    fn marching_squares_fill_concave_horseshoe_without_holes() {
        let mut owners = vec![0; 7 * 7];
        for x in 1..=5 {
            owners[6 * 7 + x] = 1;
        }
        for y in 1..=6 {
            owners[y * 7 + 1] = 1;
            owners[y * 7 + 5] = 1;
        }
        assert_mesh_covers_claimed_cells(7, 7, owners);
    }

    #[test]
    fn marching_squares_fill_narrow_bridge_without_holes() {
        let mut owners = vec![0; 8 * 5];
        for y in 1..=2 {
            for x in 1..=2 {
                owners[y * 8 + x] = 1;
            }
            for x in 5..=6 {
                owners[y * 8 + x] = 1;
            }
        }
        owners[2 * 8 + 3] = 1;
        owners[2 * 8 + 4] = 1;
        assert_mesh_covers_claimed_cells(8, 5, owners);
    }

    fn assert_mesh_covers_claimed_cells(width: u32, height: u32, owners: Vec<u8>) {
        let board = TerritoryVisual {
            width,
            height,
            cell_size: 1.0,
            owners,
            playable: vec![true; (width * height) as usize],
            ..default()
        };
        let mesh = build_chunk_mesh(&board, UVec2::ZERO);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let indices = mesh.indices().unwrap();
        for (index, owner) in board.owners.iter().enumerate() {
            if *owner == 0 {
                continue;
            }
            let x = index as u32 % width;
            let y = index as u32 / width;
            let point = dual_point(&board, x as i32, y as i32);
            let covered = indices
                .iter()
                .collect::<Vec<_>>()
                .chunks(3)
                .any(|triangle| {
                    if triangle.len() < 3 {
                        return false;
                    }
                    let a = Vec2::new(positions[triangle[0]][0], positions[triangle[0]][2]);
                    let b = Vec2::new(positions[triangle[1]][0], positions[triangle[1]][2]);
                    let c = Vec2::new(positions[triangle[2]][0], positions[triangle[2]][2]);
                    point_in_triangle(point, a, b, c)
                });
            assert!(covered, "claimed cell center {point:?} has a visual hole");
        }
    }
}
