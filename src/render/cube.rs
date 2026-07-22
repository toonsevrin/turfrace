use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::camera::ViewportSubject;

use super::{PresentationSettings, TrailVisual, materials::RenderAssets};

/// Complete render snapshot for one competitor. Simulation may update this at fixed rate;
/// presentation interpolates it in ordinary `Update`.
#[derive(Component, Debug, Clone)]
pub struct CompetitorVisual {
    pub id: u8,
    pub color_id: u8,
    pub pattern_id: u8,
    pub position: Vec2,
    pub previous_position: Vec2,
    pub heading: Vec2,
    pub human_slot: Option<u8>,
    pub alive: bool,
    pub spawn_protection: f32,
    pub is_leader: bool,
    pub awareness: f32,
}

impl Default for CompetitorVisual {
    fn default() -> Self {
        Self {
            id: 0,
            color_id: 0,
            pattern_id: 0,
            position: Vec2::ZERO,
            previous_position: Vec2::ZERO,
            heading: Vec2::NEG_Y,
            human_slot: None,
            alive: true,
            spawn_protection: 0.0,
            is_leader: false,
            awareness: 0.0,
        }
    }
}

#[derive(Component)]
pub(super) struct CompetitorProxy {
    source: Entity,
    rendered_heading: Vec2,
    lean_radians: f32,
}
#[derive(Component)]
pub(super) struct LeaderIndicator;
#[derive(Component)]
pub(super) struct SpawnShield;

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn sync_competitor_visuals(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<PresentationSettings>,
    assets: Option<Res<RenderAssets>>,
    sources: Query<(Entity, &CompetitorVisual, Option<&TrailVisual>)>,
    mut proxies: Query<
        (
            Entity,
            &mut CompetitorProxy,
            &mut Transform,
            &mut Visibility,
            &Children,
        ),
        (Without<LeaderIndicator>, Without<SpawnShield>),
    >,
    mut leader_parts: Query<
        &mut Visibility,
        (
            With<LeaderIndicator>,
            Without<SpawnShield>,
            Without<CompetitorProxy>,
        ),
    >,
    mut shields: Query<
        (&mut Visibility, &mut Transform),
        (With<SpawnShield>, Without<CompetitorProxy>),
    >,
) {
    let Some(assets) = assets else { return };
    let source_map: HashMap<Entity, (&CompetitorVisual, Option<&TrailVisual>)> = sources
        .iter()
        .map(|(entity, visual, trail)| (entity, (visual, trail)))
        .collect();
    let mut rendered = HashSet::new();
    for (entity, mut proxy, mut transform, mut root_visibility, children) in &mut proxies {
        let Some((visual, trail)) = source_map.get(&proxy.source) else {
            commands.entity(entity).despawn();
            continue;
        };
        rendered.insert(proxy.source);
        *root_visibility = if visual.alive {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let target = Vec3::new(visual.position.x, 0.0, visual.position.y);
        let alpha = 1.0 - 2.0_f32.powf(-time.delta_secs().min(0.1) / 0.045);
        transform.translation = transform.translation.lerp(target, alpha);
        let heading = visual
            .heading
            .normalize_or(proxy.rendered_heading.normalize_or(Vec2::NEG_Y));
        let heading_alpha = 1.0 - 2.0_f32.powf(-time.delta_secs().min(0.1) / 0.055);
        let (rendered_heading, rendered_turn) =
            smooth_heading(proxy.rendered_heading, heading, heading_alpha);
        let yaw = rendered_heading.x.atan2(-rendered_heading.y);
        let target_lean = if settings.reduced_motion {
            0.0
        } else {
            let angular_speed = rendered_turn / time.delta_secs().max(1.0 / 240.0);
            (angular_speed / 270.0_f32.to_radians()).clamp(-1.0, 1.0) * 5.0_f32.to_radians()
        };
        let lean_alpha = 1.0 - 2.0_f32.powf(-time.delta_secs().min(0.1) / 0.075);
        proxy.lean_radians += (target_lean - proxy.lean_radians) * lean_alpha;
        transform.rotation =
            Quat::from_rotation_y(yaw) * Quat::from_rotation_z(-proxy.lean_radians);
        proxy.rendered_heading = rendered_heading;
        for child in children.iter() {
            if let Ok(mut visibility) = leader_parts.get_mut(child) {
                *visibility = if visual.is_leader && visual.alive {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
            }
            if let Ok((mut visibility, mut shield_transform)) = shields.get_mut(child) {
                *visibility = if visual.spawn_protection > 0.0 && visual.alive {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
                let pulse = if settings.reduced_motion {
                    1.0
                } else {
                    1.0 + (time.elapsed_secs() * 5.0).sin() * 0.07
                };
                shield_transform.scale = Vec3::splat(pulse);
            }
        }
        commands
            .entity(proxy.source)
            .insert(if let Some(slot) = visual.human_slot {
                ViewportSubject {
                    slot,
                    color_id: visual.color_id,
                    position: visual.position,
                    heading,
                    trail_length: trail_length(*trail),
                    awareness: visual.awareness,
                    alive: visual.alive,
                }
            } else {
                // NPC entities do not retain stale local viewport ownership.
                ViewportSubject {
                    slot: u8::MAX,
                    color_id: visual.color_id,
                    position: visual.position,
                    heading,
                    trail_length: 0.0,
                    awareness: 0.0,
                    alive: false,
                }
            });
        if visual.human_slot.is_none() {
            commands.entity(proxy.source).remove::<ViewportSubject>();
        }
    }

    for (source, visual, trail) in &sources {
        if rendered.contains(&source) {
            continue;
        }
        spawn_proxy(&mut commands, &assets, source, visual);
        if let Some(slot) = visual.human_slot {
            commands.entity(source).insert(ViewportSubject {
                slot,
                color_id: visual.color_id,
                position: visual.position,
                heading: visual.heading,
                trail_length: trail_length(trail),
                awareness: visual.awareness,
                alive: visual.alive,
            });
        }
    }
}

fn trail_length(trail: Option<&TrailVisual>) -> f32 {
    trail.map_or(0.0, |trail| {
        trail
            .points
            .windows(2)
            .map(|pair| pair[0].distance(pair[1]))
            .sum()
    })
}

fn spawn_proxy(
    commands: &mut Commands,
    assets: &RenderAssets,
    source: Entity,
    visual: &CompetitorVisual,
) {
    let palette = visual.color_id as usize % assets.cube_materials.len();
    commands.spawn((
        Name::new(format!("Competitor {} Visual", visual.id)),
        CompetitorProxy {
            source,
            rendered_heading: visual.heading,
            lean_radians: 0.0,
        },
        Transform::from_xyz(visual.position.x, 0.0, visual.position.y),
        Visibility::default(),
        children![
            (
                Name::new("Blob Shadow"),
                Mesh3d(assets.shadow_mesh.clone()),
                MeshMaterial3d(assets.shadow.clone()),
                horizontal_disc_transform(Vec3::new(0.0, 0.112, 0.10), Vec3::new(1.0, 0.78, 1.0))
            ),
            (
                Name::new("Black Cartoon Outline"),
                Mesh3d(assets.cube_mesh.clone()),
                MeshMaterial3d(assets.black_outline.clone()),
                Transform::from_xyz(0.0, 0.79, 0.0)
            ),
            (
                Name::new("Colored Cube Core"),
                Mesh3d(assets.inner_cube_mesh.clone()),
                MeshMaterial3d(assets.cube_materials[palette].clone()),
                Transform::from_xyz(0.0, 0.79, 0.0)
            ),
            (
                Name::new("Top Profile Symbol"),
                Mesh3d(assets.icon_mesh.clone()),
                MeshMaterial3d(assets.charcoal.clone()),
                horizontal_disc_transform(Vec3::new(0.0, 1.398, 0.0), Vec3::ONE)
            ),
            (
                Name::new("Leader Crown"),
                LeaderIndicator,
                Mesh3d(assets.crown_mesh.clone()),
                MeshMaterial3d(assets.accent_materials[palette].clone()),
                Transform::from_xyz(0.0, 2.05, 0.0),
                if visual.is_leader {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                }
            ),
            (
                Name::new("Spawn Shield"),
                SpawnShield,
                Mesh3d(assets.ring_mesh.clone()),
                MeshMaterial3d(assets.accent_materials[palette].clone()),
                Transform::from_xyz(0.0, 0.72, 0.0),
                if visual.spawn_protection > 0.0 {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                }
            ),
        ],
    ));
}

fn smooth_heading(current: Vec2, target: Vec2, alpha: f32) -> (Vec2, f32) {
    let current = current.normalize_or(Vec2::NEG_Y);
    let target = target.normalize_or(current);
    let step = current.angle_to(target) * alpha.clamp(0.0, 1.0);
    (Vec2::from_angle(step).rotate(current), step)
}

fn horizontal_disc_transform(translation: Vec3, scale: Vec3) -> Transform {
    Transform::from_translation(translation)
        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
        .with_scale(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disc_meshes_are_rotated_from_xy_onto_the_field() {
        let transform = horizontal_disc_transform(Vec3::ZERO, Vec3::ONE);
        let world_normal = transform.rotation * Vec3::Z;
        assert!(world_normal.abs_diff_eq(Vec3::Y, 1.0e-6));
    }

    #[test]
    fn rendered_heading_eases_toward_authoritative_turns() {
        let current = Vec2::NEG_Y;
        let target = Vec2::X;
        let before = current.angle_to(target).abs();
        let (eased, step) = smooth_heading(current, target, 0.5);
        assert!((eased.length() - 1.0).abs() < 1.0e-6);
        assert!(step.abs() > 0.0);
        assert!(eased.angle_to(target).abs() < before);
        assert!(eased.angle_to(target).abs() > 0.0);
    }
}
