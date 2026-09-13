use bevy::prelude::*;

use crate::{camera::ViewportSubject, match_game::MatchGeneration};

use super::{
    PresentationSettings, TerritoryVisual, TrailVisual,
    materials::RenderAssets,
    territory::{FIELD_SURFACE_HEIGHT, TERRITORY_SURFACE_HEIGHT},
};

const PLAYER_CLEARANCE: f32 = 0.035;

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
    pub kills: u32,
    pub kill_streak: u32,
    pub respawn_target: Option<Vec2>,
    pub respawn_remaining: f32,
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
            kills: 0,
            kill_streak: 0,
            respawn_target: None,
            respawn_remaining: 0.0,
        }
    }
}

#[derive(Component)]
pub(crate) struct CompetitorProxy {
    pub(crate) source: Entity,
    /// Entity IDs can be reused after a restart, so source alone is not a
    /// sufficient fence against stale render proxies.
    pub(crate) generation: u64,
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
    territory: Res<TerritoryVisual>,
    generation: Option<Res<MatchGeneration>>,
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
    mut subjects: Query<&mut ViewportSubject>,
) {
    let Some(assets) = assets else { return };
    let generation = generation.map_or(0, |generation| generation.0);
    let mut rendered = [false; 12];
    for (entity, mut proxy, mut transform, mut root_visibility, children) in &mut proxies {
        if proxy.generation != generation {
            commands.entity(entity).despawn();
            continue;
        }
        let Ok((_, visual, trail)) = sources.get(proxy.source) else {
            commands.entity(entity).despawn();
            continue;
        };
        rendered[visual.id as usize] = true;
        let next_root_visibility = if visual.alive {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *root_visibility != next_root_visibility {
            *root_visibility = next_root_visibility;
        }
        let target = Vec3::new(
            visual.position.x,
            player_base_height(&territory, visual.position, trail.is_some()),
            visual.position.y,
        );
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
                let next = if visual.is_leader && visual.alive {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
                if *visibility != next {
                    *visibility = next;
                }
            }
            if let Ok((mut visibility, mut shield_transform)) = shields.get_mut(child) {
                let shield_active = visual.spawn_protection > 0.0 && visual.alive;
                let next = if shield_active {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
                if *visibility != next {
                    *visibility = next;
                }
                if shield_active {
                    let pulse = if settings.reduced_motion {
                        1.0
                    } else {
                        1.0 + (time.elapsed_secs() * 5.0).sin() * 0.07
                    };
                    shield_transform.scale = Vec3::splat(pulse);
                }
            }
        }
        let next_subject = if let Some(slot) = visual.human_slot {
            ViewportSubject {
                slot,
                color_id: visual.color_id,
                position: visual.position,
                heading,
                trail_length: trail.map_or(0.0, |trail| trail.length),
                awareness: visual.awareness,
                kill_count: visual.kills,
                kill_streak: visual.kill_streak,
                alive: visual.alive,
                respawn_target: visual.respawn_target,
                respawn_remaining: visual.respawn_remaining,
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
                kill_count: 0,
                kill_streak: 0,
                alive: false,
                respawn_target: None,
                respawn_remaining: 0.0,
            }
        };
        if let Ok(mut subject) = subjects.get_mut(proxy.source) {
            if visual.human_slot.is_some() {
                if *subject != next_subject {
                    *subject = next_subject;
                }
            } else {
                commands.entity(proxy.source).remove::<ViewportSubject>();
            }
        } else if visual.human_slot.is_some() {
            commands.entity(proxy.source).insert(next_subject);
        }
    }

    for (source, visual, trail) in &sources {
        if rendered[visual.id as usize] {
            continue;
        }
        spawn_proxy(
            &mut commands,
            &assets,
            &territory,
            generation,
            source,
            visual,
            trail.is_some(),
        );
        if let Some(slot) = visual.human_slot {
            commands.entity(source).insert(ViewportSubject {
                slot,
                color_id: visual.color_id,
                position: visual.position,
                heading: visual.heading,
                trail_length: trail.map_or(0.0, |trail| trail.length),
                awareness: visual.awareness,
                kill_count: visual.kills,
                kill_streak: visual.kill_streak,
                alive: visual.alive,
                respawn_target: visual.respawn_target,
                respawn_remaining: visual.respawn_remaining,
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_proxy(
    commands: &mut Commands,
    assets: &RenderAssets,
    territory: &TerritoryVisual,
    generation: u64,
    source: Entity,
    visual: &CompetitorVisual,
    drawing: bool,
) {
    let palette = visual.color_id as usize % assets.cube_materials.len();
    commands.spawn((
        Name::new(format!("Competitor {} Visual", visual.id)),
        CompetitorProxy {
            source,
            generation,
            rendered_heading: visual.heading,
            lean_radians: 0.0,
        },
        Transform::from_xyz(
            visual.position.x,
            player_base_height(territory, visual.position, drawing),
            visual.position.y,
        ),
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

fn player_base_height(territory: &TerritoryVisual, position: Vec2, drawing: bool) -> f32 {
    // Keep the black silhouette fully above the raised turf lip. Without a
    // small clearance the territory top clips the lower outline faces. Active
    // drawers stay at the turf traversal level over the entire path, avoiding
    // height snaps and making enemy-territory crossings read as going over it.
    let surface_height = if drawing {
        TERRITORY_SURFACE_HEIGHT
    } else {
        territory.surface_height(position)
    };
    surface_height - FIELD_SURFACE_HEIGHT + PLAYER_CLEARANCE
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
    fn proxy_source_lookup_targets_the_simulation_entity() {
        let mut world = World::new();
        let source = world
            .spawn((CompetitorVisual::default(), TrailVisual::default()))
            .id();
        let proxy = world
            .spawn(CompetitorProxy {
                source,
                generation: 0,
                rendered_heading: Vec2::NEG_Y,
                lean_radians: 0.0,
            })
            .id();
        let mut sources = world.query::<(Entity, &CompetitorVisual, Option<&TrailVisual>)>();
        let Ok((_, visual, trail)) = sources.get(&world, source) else {
            panic!("source entity should be queryable");
        };
        assert_eq!(visual.id, 0);
        assert!(trail.is_some());
        assert!(sources.get(&world, proxy).is_err());
    }

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

    #[test]
    fn cube_uses_a_stable_traversal_height_while_drawing() {
        let territory = TerritoryVisual {
            width: 1,
            height: 1,
            cell_size: 1.0,
            owners: vec![1],
            ..default()
        };
        assert_eq!(
            player_base_height(&territory, Vec2::splat(0.5), false),
            super::super::territory::TERRITORY_SURFACE_HEIGHT - FIELD_SURFACE_HEIGHT
                + PLAYER_CLEARANCE
        );
        assert_eq!(
            player_base_height(&territory, Vec2::splat(2.0), false),
            PLAYER_CLEARANCE
        );
        assert_eq!(
            player_base_height(&territory, Vec2::splat(2.0), true),
            super::super::territory::TERRITORY_SURFACE_HEIGHT - FIELD_SURFACE_HEIGHT
                + PLAYER_CLEARANCE
        );
    }
}
