use bevy::prelude::*;

use crate::{
    config::GameConfig,
    match_game::{
        Competitor, LifeState, LifeStatus, MatchGeneration, MatchPhase, MatchSession, RespawnPlan,
        SimulationPaused,
    },
    palette::PLAYER_COLORS,
    render::{FlatMaterial, PresentationSettings},
};

pub(super) struct RespawnTelegraphPlugin;

impl Plugin for RespawnTelegraphPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_respawn_telegraph_assets)
            .add_systems(Update, sync_respawn_telegraphs);
    }
}

#[derive(Resource)]
struct RespawnTelegraphAssets {
    ring: Handle<Mesh>,
    segment: Handle<Mesh>,
    ghost: Handle<Mesh>,
    materials: Vec<Handle<FlatMaterial>>,
    ghost_materials: Vec<Handle<FlatMaterial>>,
}

/// Unlike event effects, a respawn telegraph exists for the whole reservation.
/// Its source fence also makes a match restart incapable of inheriting an old
/// beacon when entity IDs happen to be reused.
#[derive(Component)]
struct RespawnTelegraph {
    source: Entity,
    generation: u64,
    position: Vec2,
    duration: f32,
    /// Presentation-local clock. It intentionally does not use global elapsed
    /// time, so pausing freezes the exact phase shown to players.
    phase: f32,
}

#[derive(Component)]
struct RespawnTelegraphRing;

#[derive(Component)]
struct RespawnTelegraphSegment {
    source: Entity,
    index: usize,
}

#[derive(Component)]
struct RespawnTelegraphGhost {
    source: Entity,
}

const RESPAWN_SEGMENT_COUNT: usize = 12;
const RESPAWN_RING_HEIGHT: f32 = 0.19;

fn setup_respawn_telegraph_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FlatMaterial>>,
) {
    commands.insert_resource(RespawnTelegraphAssets {
        // The telegraph has its own larger footprint so it reads as a reserved
        // site rather than as a capture pulse or owned territory.
        ring: meshes.add(Annulus::new(1.52, 1.62).mesh().resolution(48)),
        segment: meshes.add(Cuboid::from_size(Vec3::new(0.16, 0.055, 0.48))),
        ghost: meshes.add(Cuboid::from_size(Vec3::splat(1.28))),
        materials: PLAYER_COLORS
            .iter()
            .map(|color| {
                materials.add(FlatMaterial::transparent(
                    Color::Srgba(*color).with_alpha(0.88),
                ))
            })
            .collect(),
        ghost_materials: PLAYER_COLORS
            .iter()
            .map(|color| {
                materials.add(FlatMaterial::transparent(
                    Color::Srgba(*color).with_alpha(0.27),
                ))
            })
            .collect(),
    });
}

#[allow(clippy::too_many_arguments)]
fn sync_respawn_telegraphs(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<PresentationSettings>,
    config: Res<GameConfig>,
    paused: Option<Res<SimulationPaused>>,
    generation: Option<Res<MatchGeneration>>,
    session: Option<Res<MatchSession>>,
    assets: Res<RespawnTelegraphAssets>,
    sources: Query<(Entity, &Competitor, &LifeState, Option<&RespawnPlan>)>,
    mut telegraphs: Query<(Entity, &mut RespawnTelegraph, &Children)>,
    mut segments: Query<(&RespawnTelegraphSegment, &mut Visibility)>,
    mut ghosts: Query<(&RespawnTelegraphGhost, &mut Transform)>,
) {
    // Isolated presentation tests can omit MatchSession. In a real match,
    // only the active simulation phase is allowed to display reservations.
    let phase_allows_telegraphs = session
        .as_deref()
        .is_none_or(|session| session.phase == MatchPhase::Running);
    let generation = generation.map_or(0, |generation| generation.0);
    let paused = paused.is_some_and(|paused| paused.0);

    for (entity, mut telegraph, children) in &mut telegraphs {
        let active = phase_allows_telegraphs
            && sources
                .get(telegraph.source)
                .ok()
                .and_then(|(_, _, life, plan)| plan.map(|plan| (*life, plan)))
                .is_some_and(|(life, plan)| {
                    respawn_plan_is_active(life, Some(plan))
                        && telegraph.generation == generation
                        && plan.position == telegraph.position
                });
        if !active {
            // Despawning the parent also removes the retained visual parts;
            // unlike an event effect there is no lifetime budget to expire.
            commands.entity(entity).despawn();
            continue;
        }
        let Ok((_, _, life, Some(_plan))) = sources.get(telegraph.source) else {
            continue;
        };
        if !paused {
            telegraph.phase += time.delta_secs().min(0.05);
        }
        let progress = respawn_telegraph_progress(life.respawn_remaining, telegraph.duration);
        let visible_segments = respawn_visible_segment_count(
            life.respawn_remaining,
            telegraph.duration,
            RESPAWN_SEGMENT_COUNT,
        );
        for child in children.iter() {
            if let Ok((marker, mut visibility)) = segments.get_mut(child)
                && marker.source == telegraph.source
            {
                let next = if marker.index < visible_segments {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                if *visibility != next {
                    *visibility = next;
                }
            }
            if !paused
                && let Ok((marker, mut ghost)) = ghosts.get_mut(child)
                && marker.source == telegraph.source
            {
                let shape = respawn_ghost_scale(progress, settings.reduced_motion);
                ghost.scale = Vec3::splat(shape);
                if settings.reduced_motion {
                    ghost.rotation = Quat::IDENTITY;
                    ghost.translation.y = 0.72;
                } else {
                    ghost.rotation = Quat::from_rotation_y(telegraph.phase * 0.65);
                    ghost.translation.y = 0.72 + (telegraph.phase * 2.4).sin() * 0.06;
                }
            }
        }
    }

    if !phase_allows_telegraphs {
        return;
    }

    for (source, competitor, life, plan) in &sources {
        if !respawn_plan_is_active(*life, plan)
            || telegraphs
                .iter()
                .any(|(_, telegraph, _)| telegraph.source == source)
        {
            continue;
        }
        let plan = plan.expect("active respawn plan");
        let palette = competitor.color_id as usize % assets.materials.len();
        let root = commands
            .spawn((
                Name::new(format!("Respawn site {}", competitor.id.0)),
                RespawnTelegraph {
                    source,
                    generation,
                    position: plan.position,
                    duration: plan.duration,
                    phase: 0.0,
                },
                // Keep the root identity-scaled: its children own the
                // footprint, while the ghost must never inherit a stretch.
                Transform::from_xyz(plan.position.x, RESPAWN_RING_HEIGHT, plan.position.y),
                Visibility::Inherited,
            ))
            .id();
        commands.entity(root).with_children(|parent| {
            parent.spawn((
                Mesh3d(assets.ring.clone()),
                MeshMaterial3d(assets.materials[palette].clone()),
                RespawnTelegraphRing,
                Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::splat(config.starting_territory_radius / 1.62)),
            ));
            for index in 0..RESPAWN_SEGMENT_COUNT {
                let angle = index as f32 * std::f32::consts::TAU / RESPAWN_SEGMENT_COUNT as f32;
                parent.spawn((
                    Mesh3d(assets.segment.clone()),
                    MeshMaterial3d(assets.materials[palette].clone()),
                    RespawnTelegraphSegment { source, index },
                    Transform {
                        translation: Vec3::new(angle.cos(), 0.0, angle.sin())
                            * (config.starting_territory_radius * 1.4 / 1.62),
                        rotation: Quat::from_rotation_y(-angle),
                        ..default()
                    },
                ));
            }
            parent.spawn((
                Mesh3d(assets.ghost.clone()),
                MeshMaterial3d(assets.ghost_materials[palette].clone()),
                RespawnTelegraphGhost { source },
                Transform::from_xyz(0.0, 0.72, 0.0).with_scale(Vec3::splat(respawn_ghost_scale(
                    respawn_telegraph_progress(life.respawn_remaining, plan.duration),
                    settings.reduced_motion,
                ))),
            ));
        });
    }
}

fn respawn_plan_is_active(life: LifeState, plan: Option<&RespawnPlan>) -> bool {
    life.status == LifeStatus::Respawning
        && life.respawn_remaining > 0.0
        && plan.is_some_and(|plan| plan.duration > 0.0)
}

fn respawn_telegraph_progress(remaining: f32, duration: f32) -> f32 {
    1.0 - (remaining.max(0.0) / duration.max(0.001)).clamp(0.0, 1.0)
}

fn respawn_visible_segment_count(remaining: f32, duration: f32, segments: usize) -> usize {
    ((1.0 - respawn_telegraph_progress(remaining, duration)) * segments as f32)
        .ceil()
        .clamp(0.0, segments as f32) as usize
}

fn respawn_ghost_scale(progress: f32, reduced_motion: bool) -> f32 {
    if reduced_motion {
        1.0
    } else {
        0.34 + progress.clamp(0.0, 1.0).sqrt() * 0.66
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::prelude::{
        App, Assets, Color, Cuboid, Entity, Mesh, Time, Transform, Update, Vec2, Vec3, With,
        default,
    };

    use crate::{
        config::GameConfig,
        ids::CompetitorId,
        match_game::{
            Competitor, CompetitorKind, LifeState, LifeStatus, MatchGeneration, MatchPhase,
            MatchSession, RespawnPlan, SimulationPaused,
        },
        render::{FlatMaterial, PresentationSettings},
    };

    use super::{
        RESPAWN_SEGMENT_COUNT, RespawnTelegraph, RespawnTelegraphAssets, RespawnTelegraphGhost,
        RespawnTelegraphRing, RespawnTelegraphSegment, respawn_ghost_scale, respawn_plan_is_active,
        respawn_visible_segment_count, sync_respawn_telegraphs,
    };

    #[test]
    fn respawn_segments_track_only_the_authoritative_reserved_countdown() {
        assert_eq!(respawn_visible_segment_count(5.0, 5.0, 12), 12);
        assert_eq!(respawn_visible_segment_count(2.5, 5.0, 12), 6);
        assert_eq!(respawn_visible_segment_count(0.01, 5.0, 12), 1);
        assert_eq!(respawn_visible_segment_count(0.0, 5.0, 12), 0);
    }

    #[test]
    fn respawn_ghost_gathers_without_motion_when_reduced() {
        assert_eq!(respawn_ghost_scale(0.0, false), 0.34);
        assert_eq!(respawn_ghost_scale(1.0, false), 1.0);
        assert_eq!(respawn_ghost_scale(0.4, true), 1.0);
    }

    #[test]
    fn respawn_visual_requires_a_plan_and_a_live_countdown() {
        let life = LifeState {
            status: LifeStatus::Respawning,
            respawn_remaining: 5.0,
        };
        assert!(!respawn_plan_is_active(life, None));
        let plan = RespawnPlan {
            position: Vec2::ZERO,
            duration: 5.0,
        };
        assert!(respawn_plan_is_active(life, Some(&plan)));
        assert!(!respawn_plan_is_active(
            LifeState {
                respawn_remaining: 0.0,
                ..life
            },
            Some(&plan),
        ));
    }

    fn respawn_system_app() -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<FlatMaterial>>()
            .init_resource::<Time>()
            .insert_resource(PresentationSettings::default())
            .insert_resource(GameConfig::default())
            .insert_resource(MatchGeneration(7))
            .insert_resource(SimulationPaused(false));
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::from_size(Vec3::splat(1.0)));
        let material = app
            .world_mut()
            .resource_mut::<Assets<FlatMaterial>>()
            .add(FlatMaterial::transparent(Color::WHITE));
        app.insert_resource(RespawnTelegraphAssets {
            ring: mesh.clone(),
            segment: mesh.clone(),
            ghost: mesh,
            materials: vec![material.clone()],
            ghost_materials: vec![material],
        });
        let source = app
            .world_mut()
            .spawn((
                Competitor {
                    id: CompetitorId(0),
                    display_name: "TEST".into(),
                    kind: CompetitorKind::Human,
                    color_id: 0,
                    pattern_id: 0,
                },
                LifeState {
                    status: LifeStatus::Respawning,
                    respawn_remaining: 5.0,
                },
                RespawnPlan {
                    position: Vec2::new(2.0, -1.0),
                    duration: 5.0,
                },
            ))
            .id();
        app.add_systems(Update, sync_respawn_telegraphs);
        (app, source)
    }

    fn root_count(app: &mut App) -> usize {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<RespawnTelegraph>>()
            .iter(world)
            .count()
    }

    fn segment_count(app: &mut App) -> usize {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<RespawnTelegraphSegment>>()
            .iter(world)
            .count()
    }

    #[test]
    fn respawn_telegraph_system_owns_the_full_plan_lifecycle() {
        let (mut app, source) = respawn_system_app();
        app.update();
        assert_eq!(root_count(&mut app), 1);
        let segments = {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<RespawnTelegraphSegment>>()
                .iter(world)
                .count()
        };
        assert_eq!(segments, RESPAWN_SEGMENT_COUNT);

        app.world_mut().entity_mut(source).remove::<RespawnPlan>();
        app.update();
        assert_eq!(root_count(&mut app), 0);
        let segments = {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<RespawnTelegraphSegment>>()
                .iter(world)
                .count()
        };
        assert_eq!(segments, 0);
    }

    #[test]
    fn terminal_match_phases_remove_respawn_telegraphs() {
        let (mut app, _) = respawn_system_app();
        app.insert_resource(MatchSession {
            phase: MatchPhase::Running,
            ..default()
        });
        app.update();
        assert_eq!(root_count(&mut app), 1);

        for phase in [MatchPhase::Finished, MatchPhase::Idle, MatchPhase::Loading] {
            app.world_mut().resource_mut::<MatchSession>().phase = phase;
            app.update();
            assert_eq!(root_count(&mut app), 0);
            assert_eq!(segment_count(&mut app), 0);
        }

        app.world_mut().resource_mut::<MatchSession>().phase = MatchPhase::Running;
        app.update();
        assert_eq!(root_count(&mut app), 1);
    }

    #[test]
    fn respawn_footprint_stays_fixed_without_stretching_the_ghost() {
        let (mut app, source) = respawn_system_app();
        app.update();
        app.world_mut()
            .get_mut::<LifeState>(source)
            .unwrap()
            .respawn_remaining = 0.01;
        app.update();
        let radius = app
            .world()
            .resource::<GameConfig>()
            .starting_territory_radius;
        let world = app.world_mut();
        let root = world
            .query_filtered::<&Transform, With<RespawnTelegraph>>()
            .single(world)
            .unwrap();
        assert_eq!(root.scale, Vec3::ONE);
        let ring = world
            .query_filtered::<&Transform, With<RespawnTelegraphRing>>()
            .single(world)
            .unwrap();
        assert!((ring.scale.x * 1.62 - radius).abs() < 1e-5);
        let ghost = world
            .query_filtered::<&Transform, With<RespawnTelegraphGhost>>()
            .single(world)
            .unwrap();
        assert!(ghost.scale.x > 0.99 && ghost.scale.x <= 1.0);
        assert_eq!(ghost.scale.x, ghost.scale.z);
    }

    #[test]
    fn paused_running_respawn_telegraph_freezes_transforms() {
        let (mut app, _) = respawn_system_app();
        app.insert_resource(MatchSession {
            phase: MatchPhase::Running,
            ..default()
        });
        app.update();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(0.1));
        app.update();
        let root = {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<RespawnTelegraph>>()
                .single(world)
                .unwrap()
        };
        let before = {
            let world = app.world_mut();
            *world
                .query_filtered::<&Transform, With<RespawnTelegraphGhost>>()
                .single(world)
                .unwrap()
        };
        app.world_mut().resource_mut::<SimulationPaused>().0 = true;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(0.4));
        app.update();
        assert_eq!(
            app.world().get::<RespawnTelegraph>(root).unwrap().phase,
            0.05
        );
        let after = {
            let world = app.world_mut();
            *world
                .query_filtered::<&Transform, With<RespawnTelegraphGhost>>()
                .single(world)
                .unwrap()
        };
        assert_eq!(before, after);
    }

    #[test]
    fn respawn_relocation_replaces_the_old_telegraph_and_cancel_cleans_up() {
        let (mut app, source) = respawn_system_app();
        app.update();
        app.world_mut()
            .get_mut::<RespawnPlan>(source)
            .unwrap()
            .position = Vec2::new(-4.0, 3.0);
        app.update();
        assert_eq!(root_count(&mut app), 0);
        app.update();
        assert_eq!(root_count(&mut app), 1);
        let position = {
            let world = app.world_mut();
            world
                .query_filtered::<&RespawnTelegraph, With<RespawnTelegraph>>()
                .single(world)
                .unwrap()
                .position
        };
        assert_eq!(position, Vec2::new(-4.0, 3.0));

        app.world_mut().entity_mut(source).remove::<RespawnPlan>();
        app.update();
        assert_eq!(root_count(&mut app), 0);
    }
}
