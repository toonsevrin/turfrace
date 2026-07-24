//! Cosmetic event presentation. Effects are deliberately non-authoritative.

use bevy::prelude::*;

use crate::{
    camera::{CameraFovPulse, CameraTuning, PlayerCamera},
    match_game::KillProgress,
    palette::PLAYER_COLORS,
    render::{FlatMaterial, PresentationSettings},
};

#[derive(Message, Debug, Clone)]
pub enum VisualEffect {
    Capture {
        source: Entity,
        position: Vec2,
        color_id: u8,
        percent: f32,
    },
    Death {
        source: Entity,
        position: Vec2,
        color_id: u8,
    },
    Kill {
        source: Entity,
        position: Vec2,
        color_id: u8,
        progress: KillProgress,
    },
    Respawn {
        source: Entity,
        position: Vec2,
        color_id: u8,
    },
    LeaderChanged {
        position: Vec2,
        color_id: u8,
    },
    Victory {
        position: Vec2,
        color_id: u8,
    },
    TrailCut {
        position: Vec2,
        color_id: u8,
    },
}

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<VisualEffect>()
            .add_systems(Startup, setup_effect_assets)
            .add_systems(
                Update,
                (spawn_effects, update_effects, apply_camera_shake).chain(),
            );
    }
}

#[derive(Resource)]
struct EffectAssets {
    fragment: Handle<Mesh>,
    ring: Handle<Mesh>,
    materials: Vec<Handle<FlatMaterial>>,
}

#[derive(Component)]
struct EffectLifetime {
    remaining: f32,
    total: f32,
}
#[derive(Component)]
struct EffectVelocity(Vec3);
#[derive(Component)]
struct EffectPulse {
    base_scale: f32,
}
#[derive(Component, Default)]
struct CameraTrauma {
    amount: f32,
    phase: f32,
}

fn setup_effect_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FlatMaterial>>,
) {
    let materials = PLAYER_COLORS
        .iter()
        .map(|color| materials.add(FlatMaterial::new(Color::Srgba(*color), 0.12)))
        .collect();
    commands.insert_resource(EffectAssets {
        fragment: meshes.add(Cuboid::from_size(Vec3::splat(0.16))),
        // A flat graphic ring reads like an arcade pulse from every camera.
        // The old lit 3D torus looked like a dark crater when viewed at the
        // gameplay camera angle.
        ring: meshes.add(Annulus::new(0.64, 0.76).mesh().resolution(32)),
        materials,
    });
}

fn spawn_effects(
    mut commands: Commands,
    mut events: MessageReader<VisualEffect>,
    assets: Res<EffectAssets>,
    tuning: Res<CameraTuning>,
    settings: Res<PresentationSettings>,
    cameras: Query<(Entity, &PlayerCamera)>,
) {
    for event in events.read() {
        let (position, color_id) = match *event {
            VisualEffect::Capture {
                position, color_id, ..
            }
            | VisualEffect::Death {
                position, color_id, ..
            }
            | VisualEffect::Kill {
                position, color_id, ..
            }
            | VisualEffect::Respawn {
                position, color_id, ..
            }
            | VisualEffect::LeaderChanged { position, color_id }
            | VisualEffect::Victory { position, color_id }
            | VisualEffect::TrailCut { position, color_id } => (position, color_id),
        };
        let palette = color_id as usize % assets.materials.len();
        match *event {
            VisualEffect::Kill {
                source,
                position,
                color_id,
                progress,
            } => {
                spawn_kill_effect(
                    &mut commands,
                    &assets,
                    &tuning,
                    &settings,
                    cameras
                        .iter()
                        .find(|(_, camera)| camera.subject == source)
                        .map(|(entity, _)| entity),
                    position,
                    color_id,
                    palette,
                    progress,
                );
            }
            VisualEffect::Death { .. } | VisualEffect::TrailCut { .. } => {
                let particle_count = particle_budget(settings.quality, 14);
                for index in 0..particle_count {
                    let angle = index as f32 * std::f32::consts::TAU / particle_count as f32
                        + (index % 3) as f32 * 0.13;
                    let speed = 2.4 + (index % 5) as f32 * 0.45;
                    commands.spawn((
                        Mesh3d(assets.fragment.clone()),
                        MeshMaterial3d(assets.materials[palette].clone()),
                        Transform::from_xyz(position.x, 0.72, position.y),
                        EffectVelocity(Vec3::new(
                            angle.cos() * speed,
                            2.3 + index as f32 * 0.08,
                            angle.sin() * speed,
                        )),
                        EffectLifetime {
                            remaining: 0.55,
                            total: 0.55,
                        },
                    ));
                }
                if let VisualEffect::Death { source, .. } = *event
                    && let Some(camera) = cameras
                        .iter()
                        .find(|(_, player_camera)| player_camera.subject == source)
                        .map(|(entity, _)| entity)
                {
                    commands.entity(camera).insert(CameraTrauma {
                        amount: 1.0,
                        phase: 0.0,
                    });
                }
            }
            VisualEffect::Capture { percent, .. } => {
                // The ring and particle burst carry this cue. Text2d labels
                // are intentionally avoided here because a shared 2D camera
                // would place them on the split-screen seam rather than over
                // the world event.
                let profile = capture_effect_profile(percent);
                let particle_count = particle_budget(settings.quality, profile.particle_count);
                for index in 0..particle_count {
                    let angle = index as f32 * std::f32::consts::TAU / particle_count as f32;
                    commands.spawn((
                        Mesh3d(assets.fragment.clone()),
                        MeshMaterial3d(assets.materials[palette].clone()),
                        Transform::from_xyz(
                            position.x + angle.cos(),
                            0.25,
                            position.y + angle.sin(),
                        ),
                        EffectVelocity(Vec3::new(-angle.cos() * 1.8, 0.8, -angle.sin() * 1.8)),
                        EffectLifetime {
                            remaining: 0.42,
                            total: 0.42,
                        },
                    ));
                }
                commands.spawn((
                    Mesh3d(assets.ring.clone()),
                    MeshMaterial3d(assets.materials[palette].clone()),
                    ground_ring_transform(position).with_scale(Vec3::splat(profile.ring_scale)),
                    EffectPulse {
                        base_scale: profile.ring_scale,
                    },
                    EffectLifetime {
                        remaining: profile.duration,
                        total: profile.duration,
                    },
                ));
            }
            VisualEffect::Respawn { .. }
            | VisualEffect::LeaderChanged { .. }
            | VisualEffect::Victory { .. } => {
                let duration = if matches!(event, VisualEffect::Victory { .. }) {
                    1.8
                } else {
                    0.65
                };
                commands.spawn((
                    Mesh3d(assets.ring.clone()),
                    MeshMaterial3d(assets.materials[palette].clone()),
                    ground_ring_transform(position),
                    EffectPulse { base_scale: 1.0 },
                    EffectLifetime {
                        remaining: duration,
                        total: duration,
                    },
                ));
            }
        }
    }
}

fn ground_ring_transform(position: Vec2) -> Transform {
    Transform::from_xyz(position.x, 0.13, position.y)
        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
}

#[allow(clippy::too_many_arguments)]
fn spawn_kill_effect(
    commands: &mut Commands,
    assets: &EffectAssets,
    tuning: &CameraTuning,
    settings: &PresentationSettings,
    camera: Option<Entity>,
    position: Vec2,
    _color_id: u8,
    palette: usize,
    progress: KillProgress,
) {
    let profile = kill_effect_profile(progress);
    let particle_count = particle_budget(settings.quality, profile.particle_count);
    for index in 0..particle_count {
        let angle = index as f32 * std::f32::consts::TAU / particle_count.max(1) as f32;
        let speed = profile.burst_speed * (0.82 + (index % 4) as f32 * 0.08);
        commands.spawn((
            Mesh3d(assets.fragment.clone()),
            MeshMaterial3d(assets.materials[palette].clone()),
            Transform::from_xyz(position.x, 0.72, position.y),
            EffectVelocity(Vec3::new(
                angle.cos() * speed,
                2.45 + (index % 5) as f32 * 0.12,
                angle.sin() * speed,
            )),
            EffectLifetime {
                remaining: profile.particle_lifetime,
                total: profile.particle_lifetime,
            },
        ));
    }
    commands.spawn((
        Mesh3d(assets.ring.clone()),
        MeshMaterial3d(assets.materials[palette].clone()),
        ground_ring_transform(position).with_scale(Vec3::splat(profile.ring_scale)),
        EffectPulse {
            base_scale: profile.ring_scale,
        },
        EffectLifetime {
            remaining: profile.ring_lifetime,
            total: profile.ring_lifetime,
        },
    ));
    if !settings.reduced_motion
        && let Some(camera) = camera
    {
        commands.entity(camera).insert(CameraFovPulse {
            remaining: profile.fov_pulse_duration,
            total: profile.fov_pulse_duration,
            amount_radians: tuning.kill_fov_pulse_radians * profile.fov_scale,
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CaptureEffectProfile {
    particle_count: usize,
    ring_scale: f32,
    duration: f32,
}

fn capture_effect_profile(percent: f32) -> CaptureEffectProfile {
    let emphasis = (percent.max(0.0) / 12.0).sqrt().clamp(0.0, 1.0);
    CaptureEffectProfile {
        particle_count: 6 + (emphasis * 6.0).round() as usize,
        ring_scale: 0.42 + emphasis * 0.54,
        duration: 0.52 + emphasis * 0.34,
    }
}

fn particle_budget(quality: crate::render::GraphicsQuality, base: usize) -> usize {
    match quality {
        crate::render::GraphicsQuality::Low => (base / 2).max(1),
        crate::render::GraphicsQuality::Medium => base,
        crate::render::GraphicsQuality::High => base + 4,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct KillEffectProfile {
    tier: u8,
    particle_count: usize,
    burst_speed: f32,
    particle_lifetime: f32,
    ring_scale: f32,
    ring_lifetime: f32,
    fov_pulse_duration: f32,
    fov_scale: f32,
}

fn kill_effect_profile(progress: KillProgress) -> KillEffectProfile {
    let tier = match progress.total {
        0..=1 => 0,
        2..=3 => 1,
        4..=6 => 2,
        _ => 3,
    };
    KillEffectProfile {
        tier,
        particle_count: 10 + tier as usize * 5 + progress.streak.min(4) as usize,
        burst_speed: 2.7 + tier as f32 * 0.45,
        particle_lifetime: 0.55 + tier as f32 * 0.06,
        ring_scale: 1.0 + tier as f32 * 0.18,
        ring_lifetime: 0.62 + tier as f32 * 0.12,
        fov_pulse_duration: 0.72 + tier as f32 * 0.05,
        fov_scale: 1.0 + tier as f32 * 0.12,
    }
}

#[allow(clippy::type_complexity)]
fn update_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut effects: Query<(
        Entity,
        &mut EffectLifetime,
        &mut Transform,
        Option<&mut EffectVelocity>,
        Option<&EffectPulse>,
    )>,
) {
    let dt = time.delta_secs().min(0.05);
    for (entity, mut lifetime, mut transform, velocity, pulse) in &mut effects {
        lifetime.remaining -= dt;
        if lifetime.remaining <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let progress = 1.0 - lifetime.remaining / lifetime.total.max(0.001);
        if let Some(mut velocity) = velocity {
            transform.translation += velocity.0 * dt;
            velocity.0.y -= 8.5 * dt;
            transform.rotate(Quat::from_rotation_x(dt * 5.0));
            transform.scale = Vec3::splat((1.0 - progress).max(0.01));
        } else {
            transform.scale =
                Vec3::splat(pulse.map_or(1.0, |pulse| pulse.base_scale) * (1.0 + progress * 3.5));
        }
    }
}

fn apply_camera_shake(
    time: Res<Time>,
    settings: Res<crate::render::PresentationSettings>,
    mut cameras: Query<(&mut Transform, &mut CameraTrauma), With<PlayerCamera>>,
) {
    for (mut transform, mut trauma) in &mut cameras {
        if trauma.amount <= 0.0 {
            continue;
        }
        trauma.amount = (trauma.amount - time.delta_secs() * 4.0).max(0.0);
        if trauma.amount <= 0.0 {
            continue;
        }
        trauma.phase += time.delta_secs() * 47.0;
        if settings.reduced_motion || settings.camera_shake <= 0.0 {
            continue;
        }
        let strength = trauma.amount * trauma.amount * settings.camera_shake * 0.18;
        transform.translation.x += trauma.phase.sin() * strength;
        transform.translation.z += (trauma.phase * 1.37).sin() * strength;
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{Vec2, Vec3};

    use crate::match_game::KillProgress;

    use super::{
        capture_effect_profile, ground_ring_transform, kill_effect_profile, particle_budget,
    };

    #[test]
    fn effect_rings_lie_flat_on_the_field() {
        let transform = ground_ring_transform(Vec2::new(2.0, -3.0));
        assert!((transform.rotation * Vec3::Z).abs_diff_eq(Vec3::Y, 1.0e-6));
        assert_eq!(transform.translation, Vec3::new(2.0, 0.13, -3.0));
    }

    #[test]
    fn kill_effects_escalate_without_unbounded_particle_growth() {
        let starter = kill_effect_profile(KillProgress {
            total: 1,
            streak: 1,
        });
        let rampage = kill_effect_profile(KillProgress {
            total: 5,
            streak: 4,
        });
        let unstoppable = kill_effect_profile(KillProgress {
            total: 100,
            streak: 100,
        });
        assert!(rampage.particle_count > starter.particle_count);
        assert!(unstoppable.particle_count <= 29);
        assert!(unstoppable.ring_scale > rampage.ring_scale);
    }

    #[test]
    fn low_quality_reduces_effect_particle_budget() {
        assert!(particle_budget(crate::render::GraphicsQuality::Low, 14) < 14);
        assert_eq!(
            particle_budget(crate::render::GraphicsQuality::Medium, 14),
            14
        );
        assert!(particle_budget(crate::render::GraphicsQuality::High, 14) > 14);
    }

    #[test]
    fn larger_captures_get_stronger_but_bounded_pulses() {
        let small = capture_effect_profile(0.2);
        let large = capture_effect_profile(20.0);
        let enormous = capture_effect_profile(10_000.0);
        assert!(large.particle_count > small.particle_count);
        assert!(large.ring_scale > small.ring_scale);
        assert!(large.duration > small.duration);
        assert_eq!(large, enormous);
        assert!(enormous.particle_count <= 12);
    }
}
